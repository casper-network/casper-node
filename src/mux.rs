//! Stream multiplexing
//!
//! Multiplexes multiple sinks into a single one, allowing no more than one frame to be buffered for
//! each to avoid starvation or flooding.

use std::{
    fmt::Debug,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{Sink, SinkExt};

use crate::ImmediateFrame;

pub type ChannelPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, F>;

/// A waiting list handing out turns to interested participants in round-robin fashion.
///
/// The list is set up with a set of `n` participants labelled from `0..(n-1)` and no active
/// participant. Any participant can attempt to acquire the lock by calling the `try_acquire`
/// function.
///
/// If the lock is currently unavailable, the participant will be put in a wait queue and is
/// guaranteed a turn "in order" at some point when it calls `try_acquire` again. If a participant
/// has not registered interest in obtaining the lock their turn is skipped.
///
/// Once work has been completed, the lock must manually be released using the `end_turn`
///
/// This "lock" differs from `Mutex` in multiple ways:
///
/// * Mutable access required: Counterintuitively this lock needs to be wrapped in a `Mutex` to
///   guarding access to its internals.
/// * No notifications/waiting: There is no way to wait for the lock to become available, rather it
///   is assumed participants get an external notification indication that the lock might now be
///   available.
/// * Advisory: No actual access control is enforced by the type system, rather it is assumed that
///   clients are well behaved and respect the lock.
///   (TODO: We can possibly put a ghost cell here to enforce it)
/// * Fixed set of participants: The total set of participants must be specified in advance.
#[derive(Debug)]
struct RoundRobinAdvisoryLock {
    /// The currently active lock holder.
    active: Option<u8>,
    /// Participants wanting to take a turn.
    waiting: Vec<bool>,
}

impl RoundRobinAdvisoryLock {
    /// Creates a new round robin advisory lock with the given number of participants.
    pub fn new(num_participants: u8) -> Self {
        let mut waiting = Vec::new();
        waiting.resize(num_participants as usize, false);

        Self {
            active: None,
            waiting,
        }
    }

    /// Tries to take a turn on the wait list.
    ///
    /// If it is our turn, or if the wait list was empty, marks us as active and returns `true`.
    /// Otherwise, marks `me` as wanting a turn and returns `false`.
    ///
    /// # Safety
    ///
    /// A participant MUST NOT give up on calling `try_acquire` once it has called it once, as the
    /// lock will ultimately prevent any other participant from acquiring it while the interested is
    /// registered.
    ///
    /// # Panics
    ///
    /// Panics if `me` is not a participant in the initial set of participants.
    fn try_acquire(&mut self, me: u8) -> bool {
        debug_assert!(
            self.waiting.len() as u8 > me,
            "participant out of bounds in advisory lock"
        );

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
    fn release(&mut self, me: u8) {
        assert_eq!(
            self.active,
            Some(me),
            "tried to release unacquired advisory lock"
        );

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

/// A frame multiplexer.
///
/// Typically the multiplexer is not used directly, but used to spawn multiplexing handles.
struct Multiplexer<S> {
    wait_list: Mutex<RoundRobinAdvisoryLock>,
    sink: Mutex<Option<S>>,
}

impl<S> Multiplexer<S> {
    /// Create a handle for a specific multiplexer channel on this multiplexer.
    ///
    /// # Safety
    ///
    /// This function **must not** be called multiple times on the same `Multiplexer` with the same
    /// `channel` value.
    pub fn get_channel_handle(self: Arc<Self>, channel: u8) -> MultiplexerHandle<S> {
        MultiplexerHandle {
            multiplexer: self,
            slot: channel,
        }
    }
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
        // Required invariant: For any channel there is only one handle, thus we are the only one
        // writing to the `waiting[n]` atomic bool.

        // Try to grab a slot on the wait list (will put us into the queue if we don't get one).
        let our_turn = self
            .multiplexer
            .wait_list
            .lock()
            .expect("TODO handle poisoning")
            .try_acquire(self.slot);

        // At this point, we no longer hold the `wait_list` lock.

        if !our_turn {
            Poll::Pending
        } else {
            // We are now active, check if the sink is ready.
            match *self.multiplexer.sink.lock().expect("TODO: Lock Poisoning") {
                Some(ref mut sink_ref) => sink_ref.poll_ready_unpin(cx),
                None => todo!("handle closed multiplexer"),
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: F) -> Result<(), Self::Error> {
        let prefixed = ImmediateFrame::from(self.slot).chain(item);

        let mut guard = self.multiplexer.sink.lock().expect("TODO: Lock Poisoning");

        match *guard {
            Some(ref mut sink_ref) => sink_ref.start_send_unpin(prefixed),
            None => todo!("handle closed multiplexer"),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Obtain the flush result, then release the sink lock.
        let flush_result = {
            let mut guard = self.multiplexer.sink.lock().expect("TODO: Lock Poisoning");

            match *guard {
                Some(ref mut sink) => sink.poll_flush_unpin(cx),
                None => todo!("TODO: MISSING SINK"),
            }
        };

        match flush_result {
            Poll::Ready(Ok(())) => {
                // Acquire wait list lock to update it.
                self.multiplexer
                    .wait_list
                    .lock()
                    .expect("TODO: Lock poisoning")
                    .release(self.slot);

                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(_)) => {
                todo!("handle error")
            }

            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Simply close? Note invariants, possibly checking them in debug mode.
        todo!()
    }
}
