//! Components
//!
//! Docs to be written, sorry.

pub(crate) mod small_network;
pub(crate) mod storage;

use crate::effect::{Effect, Multiple};

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
pub(crate) trait Component {
    /// Event associated with `Component`.
    ///
    /// The event type that is handled by the component.
    type Event;

    /// Processes an event, outputting zero or more effects.
    ///
    /// This function must not ever perform any blocking or CPU intensive work, as it is expected
    /// to return very quickly.
    fn handle_event(&mut self, event: Self::Event) -> Multiple<Effect<Self::Event>>;
}
