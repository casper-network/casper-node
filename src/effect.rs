//! Effects subsystem.
//!
//! Effects describe things that the creator of the effect intends to happen,
//! producing a value upon completion. They are, in fact, futures.
//!
//! A boxed, pinned future returning an event is called an effect and typed as an `Effect<Ev>`,
//! where `Ev` is the event's type.
//!
//! ## Using effects
//!
//! To create an effect, an events factory is used that implements one or more of the factory
//! traits of this module. For example, given an events factory `eff`, we can create a
//! `set_timeout` future and turn it into an effect:
//!
//! ```
//! # use std::time;
//! use crate::effect::EffectExt;
//!
//! enum Event {
//!     ThreeSecondsElapsed(time::Duration)
//! }
//!
//! eff.set_timeout(time::Duration::from_secs(3))
//!    .event(Event::ThreeSecondsElapsed)
//! ```
//!
//! This example will produce an effect that, after three seconds, creates an
//! `Event::ThreeSecondsElapsed`. Note that effects do nothing on their own, they need to be passed
//! to the `Reactor` (see `reactor` module) to be executed.
//!
//! ## Chaining futures and effects
//!
//! Effects are built from futures, which can be combined before being finalized
//! into an effect. However, only one effect can be created as the end result
//! of such a chain.
//!
//! It is possible to create an effect from multiple effects being run in parallel using `.also`:
//!
//! ```
//! # use std::time;
//! use crate::effect::{EffectExt, EffectAlso};
//!
//! enum Event {
//!     ThreeSecondsElapsed(time::Duration),
//!     FiveSecondsElapsed(time::Duration),
//! }
//!
//! // This effect produces a single event after five seconds:
//! eff.set_timeout(time::Duration::from_secs(3))
//!    .then(|_| eff.set_timeout(time::Duration::from_secs(2))
//!    .event(Event::FiveSecondsElapsed);
//!
//! // Here, two effects are run in parallel, resulting in two events:
//! eff.set_timeout(time::Duration::from_secs(3))
//!    .event(Event::ThreeSecondsElapsed)
//!    .also(eff.set_timeout(time::Duration::from_secs(5))
//!             .event(Event::FiveSecondsElapsed));
//! ```
//!
//! ## Arbitrary effects
//!
//! While it is technically possible to turn any future into an effect, it is advisable to only use
//! the effects explicitly listed in this module through traits to create them. Post-processing on
//! effects to turn them into events should also be kept brief.

use crate::util::zero_one_many::ZeroOneMany;
use futures::future::BoxFuture;
use futures::FutureExt;
use std::future::Future;
use std::time;

/// Effect type.
///
/// Effects are just boxed futures that produce one or more events.
pub type Effect<T> = BoxFuture<'static, ZeroOneMany<T>>;

/// Effect extension for futures.
///
/// Used to convert futures into actual effects.
pub trait EffectExt: Future + Send {
    /// Finalize a future into an effect that returns an event.
    ///
    /// The passed in function `f` is used to translate the resulting value from an effect into
    fn event<U, F>(self, f: F) -> Effect<U>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
        Self: Sized;

    /// Finalize a future into an effect that runs but drops the result.
    fn ignore<Ev>(self) -> Effect<Ev>;
}

/// Parallel execution combinator for finalized effects.
pub trait EffectAlso: Future + Send {
    /// Create new effect that runs multiple effects in parallel.
    fn also(self, other: Self) -> Self;
}

impl<T: ?Sized> EffectExt for T
where
    T: Future + Send + 'static + Sized,
{
    fn event<U, F>(self, f: F) -> Effect<U>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
    {
        self.map(f).map(ZeroOneMany::One).boxed()
    }

    fn ignore<Ev>(self) -> Effect<Ev> {
        self.map(|_| ZeroOneMany::Zero).boxed()
    }
}

impl<T> EffectAlso for Effect<T>
where
    T: Send + 'static,
{
    fn also(self, other: Self) -> Self {
        async move {
            let (a, b) = futures::join!(self, other);
            a.combine_unordered(b)
        }
        .boxed()
    }
}

/// Core effects.
pub trait Core {
    /// Do not do anything.
    ///
    /// Immediately completes, can be used to trigger an event.
    fn do_nothing(self) -> BoxFuture<'static, ()>;

    /// Set a timeout.
    ///
    /// Once the timeout fires, it will return the actual elapsed time since the execution (not
    /// creation!) of this effect. Event loops typically execute effects right after a called event
    /// handling function completes.
    fn set_timeout(self, timeout: time::Duration) -> BoxFuture<'static, time::Duration>;
}
