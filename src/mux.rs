//! Stream multiplexing
//!
//! Multiplexes multiple sinks into a single one, allowing no more than one frame to be buffered for
//! each to avoid starvation or flooding.

// Have a locked

use std::{
    fmt::Debug,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{Future, Sink, SinkExt};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::{PollSendError, PollSender};

use crate::{error::Error, ImmediateFrame};

pub type ChannelPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, F>;

type SendTaskPayload<F> = (OwnedSemaphorePermit, ChannelPrefixedFrame<F>);

// IDEA: Put Arc<AtomicBools> in a vec and flip, along with a count?

const EMPTY: u8 = 0xFF;

#[derive(Debug)]
struct RoundRobinWaitList {
    active: Option<u8>,
    waiting: Vec<bool>,
}

impl RoundRobinWaitList {
    /// Tries to take a turn on the wait list.
    ///
    /// If it is our turn, or if the wait list was empty, marks us as active and returns `true`.
    /// Otherwise, marks `me` as wanting a turn and returns `false`.
    fn try_take_turn(&mut self, me: u8) -> bool {
        if let Some(active) = self.active {
            if active == me {
                return true;
            }

            // Someone is already sending, mark us as interested.
            self.waiting[me as usize] = true;
            return false;
        }

        // If we reached this, no one was sending, mark us as active.
        self.active = Some(me);
        true
    }

    /// Finish taking a turn.
    ///
    /// This function must only be called if `try_take_turn` returned `true` and the wait has not
    /// been modified in the meantime.
    ///
    /// # Panic
    ///
    /// Panics if the active turn was modified in the meantime.
    fn end_turn(&mut self, me: u8) {
        assert_eq!(self.active, Some(me));

        // We finished our turn, mark us as no longer interested.
        self.waiting[me as usize] = false;

        // Now determine the next slot in line.
        for offset in 0..self.waiting.len() {
            let idx = (me as usize + offset) % self.waiting.len();
            if self.waiting[idx] {
                self.active = Some(idx as u8);
                return;
            }
        }

        // We found no slot, so we're inactive.
        self.active = None;
    }
}

struct Multiplexer<S> {
    wait_list: Mutex<RoundRobinWaitList>,
    sink: Mutex<Option<S>>,
}

struct MultiplexerHandle<S> {
    multiplexer: Arc<Multiplexer<S>>,
    slot: u8,
}

impl<F, S> Sink<F> for MultiplexerHandle<S>
where
    S: Sink<ChannelPrefixedFrame<F>> + Unpin,
    F: Buf,
{
    type Error = <S as Sink<ChannelPrefixedFrame<F>>>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let slot = self.slot;

        // Try to grab a slot on the wait list (will put us into the queue if we don't get one).
        if !self
            .multiplexer
            .wait_list
            .lock()
            .expect("TODO handle poisoning")
            .try_take_turn(self.slot)
        {
            Poll::Pending
        } else {
            // We are now active, check if the sink is ready.
        }

        // Our first task is to determine whether our channel is currently active, or if we can
        // activate it ourselves due to it being empty.
        let active = self.multiplexer.active_slot.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |current| {
                if current == EMPTY || current == slot {
                    return Some(slot);
                }
                None
            },
        );

        match active {
            Ok(_) => {
                // Required invariant: For any channel there is only one handle, thus we are the
                // only one writing to the `waiting[n]` atomic bool.

                // We are the only handle allowed to send right now.
                let ready_poll_result =
                    match *self.multiplexer.sink.lock().expect("TODO: Lock Poisoning") {
                        Some(ref mut sink_ref) => sink_ref.poll_ready_unpin(cx),
                        None => todo!("handle closed multiplexer"),
                    };

                match ready_poll_result {
                    Poll::Ready(Ok(())) => {
                        self.multiplexer.waiting[self.slot as usize].store(false, Ordering::SeqCst);
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Err(_err)) => todo!("sink closed"),
                    Poll::Pending => Poll::Pending,
                }
            }
            Err(_) => {
                // We need to wait until the channel is either empty or our slot is picked. First,
                // mark us as interested in the wait list.
                self.multiplexer.waiting[self.slot as usize].store(true, Ordering::SeqCst);

                // We still need to wait our turn.
                return Poll::Pending;
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let mut guard = self.multiplexer.sink.lock().expect("TODO: Lock Poisoning");
        let prefixed = ImmediateFrame::from(self.slot).chain(item);
        match *guard {
            Some(ref mut sink_ref) => sink_ref.start_send_unpin(prefixed),
            None => todo!("handle closed multiplexer"),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut guard = self.multiplexer.sink.lock().expect("TODO: Lock Poisoning");
        match *guard {
            Some(ref mut sink_ref) => match sink_ref.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => {
                    // We finished sending our item. We now iterate through the waitlist.
                }
                Poll::Ready(Err(_err)) => todo!("handle sink error"),
                Poll::Pending => Poll::Pending,
            },
            None => todo!("handle closed multiplexer"),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}

#[derive(Debug)]
struct Muxtable<F> {
    /// A collection of synchronization primitives indicating whether or not a message is currently
    /// being processed for a specific subchannel.
    // Note: A manual `Sink` implementation could probably poll an `AtomicBool` here and on failure
    //       register to be woken up again, but for now we have to make do with the semaphore here.
    slots: Vec<Arc<Semaphore>>,
    /// Sender where outgoing frames go.
    sender: mpsc::Sender<SendTaskPayload<F>>,
}

struct Muxhandle<F> {
    table: Arc<Muxtable<F>>,
}

impl<F> Muxtable<F>
where
    F: Buf + Send + Debug + 'static,
{
    pub fn new<S>(num_slots: u8, mut sink: S) -> (impl Future<Output = ()>, Self)
    where
        S: Sink<ChannelPrefixedFrame<F>> + Unpin,
    {
        let (sender, mut receiver) = mpsc::channel(num_slots as usize);

        let send_task = async move {
            let mut pinned_sink = Pin::new(&mut sink);

            while let Some((_permit, channel_frame)) = receiver.recv().await {
                pinned_sink
                    .send(channel_frame)
                    .await
                    .unwrap_or_else(|_sink_err| {
                        todo!("handle sink error, closing all semaphores as well")
                    });
                // Permit will automatically be dropped once the loop iteration finishes.
            }
        };
        let muxtable = Muxtable {
            slots: (0..(num_slots as usize))
                .into_iter()
                .map(|_| Arc::new(Semaphore::new(1)))
                .collect(),
            sender,
        };

        (send_task, muxtable)
    }

    pub fn muxed_channel_handle(
        &self,
        channel: u8,
    ) -> impl Sink<F, Error = Error<PollSendError<SendTaskPayload<F>>>> {
        let poll_sender = PollSender::new(self.sender.clone());
        let slot = self.slots[channel as usize].clone(); // TODO: Error if slot missing.

        poll_sender.with(move |frame| {
            let fut_slot = slot.clone();
            async move {
                let permit = fut_slot.acquire_owned().await.expect("TODO");
                Ok((permit, ImmediateFrame::from(channel).chain(frame)))
            }
        })
    }
}
