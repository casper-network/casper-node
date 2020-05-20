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
//! To create an effect, an events factory is used that implements one or more of the factory traits
//! of this module. For example, given an events factory `events_factory`, we can create a
//! `set_timeout` future and turn it into an effect:
//!
//! ```
//! use std::time::Duration;
//! use casper_node::effect::{Core, EffectExt};
//!
//! # struct EffectBuilder {}
//! #
//! # impl Core for EffectBuilder {
//! #     fn immediately(self) -> futures::future::BoxFuture<'static, ()> {
//! #         Box::pin(async {})
//! #     }
//! #
//! #     fn set_timeout(self, timeout: Duration) -> futures::future::BoxFuture<'static, Duration> {
//! #         Box::pin(async move {
//! #             let then = std::time::Instant::now();
//! #             tokio::time::delay_for(timeout).await;
//! #             std::time::Instant::now() - then
//! #         })
//! #     }
//! # }
//! # let events_factory = EffectBuilder {};
//! #
//! enum Event {
//!     ThreeSecondsElapsed(Duration)
//! }
//!
//! events_factory
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

use std::{future::Future, time::Duration};

use futures::{future::BoxFuture, FutureExt};
use smallvec::{smallvec, SmallVec};

/// Boxed futures that produce one or more events.
pub type Effect<Ev> = BoxFuture<'static, Multiple<Ev>>;

/// Intended to hold a small collection of effects.
///
/// Stored in a `SmallVec` to avoid allocations in case there are less than three items grouped. The
/// size of two items is chosen because one item is the most common use case, and large items are
/// typically boxed. In the latter case two pointers and one enum variant discriminator is almost
/// the same size as an empty vec, which is two pointers.
pub type Multiple<T> = SmallVec<[T; 2]>;

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

/// Core effects.
pub trait Core {
    /// Immediately completes without doing anything.
    ///
    /// Can be used to trigger an event.
    fn immediately(self) -> BoxFuture<'static, ()>;

    /// Sets a timeout.
    ///
    /// Once the timeout fires, it will return the actual elapsed time since the execution (not
    /// creation!) of this effect. Event loops typically execute effects right after a called event
    /// handling function completes.
    fn set_timeout(self, timeout: Duration) -> BoxFuture<'static, Duration>;
}
