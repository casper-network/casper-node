//! Stream multiplexing
//!
//! Multiplexes multiple sink into a single one, allowing no more than one frame to be buffered for
//! each to avoid starving or flooding.

use std::{fmt::Debug, pin::Pin, sync::Arc};

use bytes::Buf;
use futures::{Future, Sink, SinkExt};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::{PollSendError, PollSender};

use crate::{error::Error, ImmediateFrame};

pub type ChannelPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, F>;

type SendTaskPayload<F> = (OwnedSemaphorePermit, ChannelPrefixedFrame<F>);

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
