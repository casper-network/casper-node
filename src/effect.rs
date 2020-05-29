//! Effects subsystem.
//!
//! Effects describe things that the creator of the effect intends to happen, producing a value upon
//! completion. They are, in fact, futures.
//!
//! A boxed, pinned future returning an event is called an effect and typed as an `Effect<Ev>`,
//! where `Ev` is the event's type.
//!
//! ## Using effects
//!
//! To create an effect, an `EffectBuilder` will be passed in from the relevant reactor. For
//! example, given an effect builder `effect_builder`, we can create a `set_timeout` future and turn
//! it into an effect:
//!
//! ```ignore
//! use std::time::Duration;
//! use casper_node::effect::EffectExt;
//!
//! enum Event {
//!     ThreeSecondsElapsed(Duration)
//! }
//!
//! effect_builder
//!     .set_timeout(Duration::from_secs(3))
//!     .event(Event::ThreeSecondsElapsed);
//! ```
//!
//! This example will produce an effect that, after three seconds, creates an
//! `Event::ThreeSecondsElapsed`. Note that effects do nothing on their own, they need to be passed
//! to a [`reactor`](../reactor/index.html) to be executed.
//!
//! ## Arbitrary effects
//!
//! While it is technically possible to turn any future into an effect, it is advisable to only use
//! the effects explicitly listed in this module through traits to create them. Post-processing on
//! effects to turn them into events should also be kept brief.

use std::{
    fmt::{self, Debug, Display, Formatter},
    future::Future,
    time::{Duration, Instant},
};

use futures::{channel::oneshot, future::BoxFuture, FutureExt};
use smallvec::{smallvec, SmallVec};
use tracing::error;

/// A boxed future that produces one or more events.
pub type Effect<Ev> = BoxFuture<'static, Multiple<Ev>>;

/// Intended to hold a small collection of [`Effect`](type.Effect.html)s.
///
/// Stored in a `SmallVec` to avoid allocations in case there are less than three items grouped. The
/// size of two items is chosen because one item is the most common use case, and large items are
/// typically boxed. In the latter case two pointers and one enum variant discriminator is almost
/// the same size as an empty vec, which is two pointers.
pub type Multiple<T> = SmallVec<[T; 2]>;

/// A boxed closure which returns an [`Effect`](type.Effect.html).
pub struct Responder<T, Ev>(Box<dyn FnOnce(T) -> Effect<Ev> + Send>);

impl<T: 'static + Send, Ev> Responder<T, Ev> {
    fn new(sender: oneshot::Sender<T>) -> Self {
        Responder(Box::new(move |value| {
            async move {
                if sender.send(value).is_err() {
                    error!("could not send response to request down oneshot channel")
                }
                smallvec![]
            }
            .boxed()
        }))
    }
}

impl<T, Ev> Responder<T, Ev> {
    /// Invoke the wrapped closure, passing in `data`.
    pub fn call(self, data: T) -> Effect<Ev> {
        self.0(data)
    }
}

impl<T, Ev> Debug for Responder<T, Ev> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "responder")
    }
}

impl<T, Ev> Display for Responder<T, Ev> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, formatter)
    }
}

/// Effect extension for futures, used to convert futures into actual effects.
pub trait EffectExt: Future + Send {
    /// Finalizes a future into an effect that returns an event.
    ///
    /// The function `f` is used to translate the returned value from an effect into an event.
    fn event<U, F>(self, f: F) -> Multiple<Effect<U>>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
        Self: Sized;

    /// Finalizes a future into an effect that runs but drops the result.
    fn ignore<Ev>(self) -> Multiple<Effect<Ev>>;
}

/// Effect extension for futures, used to convert futures returning a `Result` into two different
/// effects.
pub trait EffectResultExt {
    /// The type the future will return if `Ok`.
    type Value;
    /// The type the future will return if `Err`.
    type Error;

    /// Finalizes a future returning a `Result` into two different effects.
    ///
    /// The function `f_ok` is used to translate the returned value from an effect into an event,
    /// while the function `f_err` does the same for a potential error.
    fn result<U, F, G>(self, f_ok: F, f_err: G) -> Multiple<Effect<U>>
    where
        F: FnOnce(Self::Value) -> U + 'static + Send,
        G: FnOnce(Self::Error) -> U + 'static + Send,
        U: 'static;
}

impl<T: ?Sized> EffectExt for T
where
    T: Future + Send + 'static + Sized,
{
    fn event<U, F>(self, f: F) -> Multiple<Effect<U>>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
    {
        smallvec![self.map(f).map(|item| smallvec![item]).boxed()]
    }

    fn ignore<Ev>(self) -> Multiple<Effect<Ev>> {
        smallvec![self.map(|_| Multiple::new()).boxed()]
    }
}

impl<T: ?Sized, V, E> EffectResultExt for T
where
    T: Future<Output = Result<V, E>> + Send + 'static + Sized,
{
    type Value = V;
    type Error = E;

    fn result<U, F, G>(self, f_ok: F, f_err: G) -> Multiple<Effect<U>>
    where
        F: FnOnce(V) -> U + 'static + Send,
        G: FnOnce(E) -> U + 'static + Send,
        U: 'static,
    {
        smallvec![self
            .map(|result| result.map_or_else(f_err, f_ok))
            .map(|item| smallvec![item])
            .boxed()]
    }
}

use crate::reactor::{EventQueueHandle, QueueKind, Reactor};

/// A builder for [`Effect`](type.Effect.html)s.
///
/// Provides methods allowing the creation of effects which need scheduled on the reactor's event
/// queue, without giving direct access to this queue.
#[derive(Debug)]
pub struct EffectBuilder<R: Reactor, Ev> {
    event_queue_handle: EventQueueHandle<R, Ev>,
    queue_kind: QueueKind,
}

// Implement `Clone` and `Copy` manually, as `derive` will make it depend on `R` and `Ev` otherwise.
impl<R: Reactor, Ev> Clone for EffectBuilder<R, Ev> {
    fn clone(&self) -> Self {
        EffectBuilder {
            event_queue_handle: self.event_queue_handle,
            queue_kind: self.queue_kind,
        }
    }
}

impl<R: Reactor, Ev> Copy for EffectBuilder<R, Ev> {}

impl<R: Reactor, Ev> EffectBuilder<R, Ev> {
    pub(crate) fn new(event_queue_handle: EventQueueHandle<R, Ev>, queue_kind: QueueKind) -> Self {
        EffectBuilder {
            event_queue_handle,
            queue_kind,
        }
    }

    /// Sets a timeout.
    pub async fn set_timeout(self, timeout: Duration) -> Duration {
        let then = Instant::now();
        tokio::time::delay_for(timeout).await;
        Instant::now() - then
    }

    /// Creates a request and response pair.
    ///
    /// This creates and enqueues a request event by invoking the provided `create_request_event`
    /// closure, having first created the responder required in the form of a oneshot channel.
    pub async fn make_request<T, F>(self, create_request_event: F) -> T
    where
        T: 'static + Send,
        F: FnOnce(Responder<T, Ev>) -> Ev,
    {
        // Prepare a channel.
        let (sender, receiver) = oneshot::channel();

        // Create response function.
        let responder = Responder::new(sender);

        // Now inject the request event into the event loop.
        let request_event = create_request_event(responder);
        self.event_queue_handle
            .schedule(request_event, self.queue_kind)
            .await;

        receiver.await.unwrap_or_else(|err| {
            // The channel should never be closed, ever.
            error!(%err, "request oneshot closed, this should not happen");
            unreachable!()
        })
    }
}
