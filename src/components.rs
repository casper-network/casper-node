//! Components
//!
//! Components are the building blocks of the whole application, wired together inside a reactor.
//! Each component has a unified interface, expressed by the `Component` trait.
pub(crate) mod api_server;
pub(crate) mod consensus;
pub(crate) mod deploy_broadcaster;
// The  `in_memory_network` is public for use in doctests.
pub mod in_memory_network;
pub(crate) mod pinger;
pub(crate) mod small_network;
pub(crate) mod storage;

use rand::Rng;

use crate::effect::{Effect, EffectBuilder, Multiple};

/// Core Component.
///
/// A component implements a state machine, not unlike a [Mealy
/// automaton](https://en.wikipedia.org/wiki/Mealy_machine). Its inputs are `Event`s, allowing it to
/// perform work whenever an event is received, outputting `Effect`s each time it is called.
///
/// # Error and halting states
///
/// Components in general are expected to be able to handle every input (`Event`) in every state.
/// Invalid inputs are supposed to be discarded, and the machine is expected to recover from any
/// recoverable error states by itself.
///
/// If a fatal error occurs that is not recoverable, the reactor should be notified instead.
///
/// # Component events and reactor events
///
/// Each component has two events related to it: An associated `Event` and a reactor event (`REv`).
/// The `Event` type indicates what type of event a component accepts, these are typically event
/// types specific to the component.
///
/// Components place restrictions on reactor events (`REv`s), indicating what kind of effects they
/// need to be able to produce to operate.
pub trait Component<REv> {
    /// Event associated with `Component`.
    ///
    /// The event type that is handled by the component.
    type Event;

    /// Processes an event, outputting zero or more effects.
    ///
    /// This function must not ever perform any blocking or CPU intensive work, as it is expected
    /// to return very quickly.
    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>>;
}
