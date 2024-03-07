//! Components subsystem.
//!
//! Components are the building blocks for the application and wired together inside a
//! [reactor](crate::reactor). Each component has a unified interface, expressed by the
//! [`Component`] trait.
//!
//! # Events
//!
//! Every component defines a set of events it can process, expressed through the
//! [`Component::Event`] associated type. If an event that originated outside the component is to be
//! handled (e.g. a request or announcement being handled), a `From<OutsideEvent> for
//! ComponentEvent` implementation must be added (see component vs reactor event section below).
//!
//! A typical cycle for components is to receive an event, either originating from the outside, or
//! as the result of an effect created by the component. This event is processed in the
//! [`handle_event`](Component::handle_event) function, potentially returning effects that may
//! produce new events.
//!
//! # Error and halting states
//!
//! Components in general are expected to be able to handle every input (that is every
//! [`Component::Event`]) in every state. Unexpected inputs should usually be logged and discarded,
//! if possible, and the component is expected to recover from error states by itself.
//!
//! When a recovery is not possible, the [`fatal!`](crate::fatal!) macro should be used to produce
//! an effect that will shut down the system.
//!
//! # Component events and reactor events
//!
//! It is easy to confuse the components own associated event ([`Component::Event`]) and the
//! so-called "reactor event", often written `REv` (see [`effects`](crate::effect) for details on
//! the distinctions).
//!
//! A component's own event defines what sort of events it produces purely for internal use, and
//! also which unbound events it can accept. **Acceptance of external events** is expressed by
//! implementing a `From` implementation for the unbound, i.e. a component that can process
//! `FooAnnouncement` and a `BarRequest` will have to `impl From<FooAnnouncement> for Event` and
//! `impl From<BarRequest>`, with `Event` being the event named as [`Component::Event`].
//!
//! Since components are usually not specific to only a single reactor, they have to implement
//! `Component<REv>` for a variety of reactor events (`REv`). A component can **demand that the
//! reactor provides a set of capabilities** by requiring `From`-implementations on the `REv`, e.g.
//! by restricting the `impl Component<REv>` by `where REv: From<Baz>`. The concrete requirement
//! will usually be dictated by a restriction on a method on an
//! [`EffectBuilder`](crate::effect::EffectBuilder).

pub(crate) mod binary_port;
pub(crate) mod block_accumulator;
pub(crate) mod block_synchronizer;
pub(crate) mod block_validator;
pub mod consensus;
pub mod contract_runtime;
pub(crate) mod deploy_buffer;
pub(crate) mod diagnostics_port;
pub(crate) mod event_stream_server;
pub(crate) mod fetcher;
pub(crate) mod gossiper;
// The `in_memory_network` is public for use in doctests.
#[cfg(test)]
pub mod in_memory_network;
pub(crate) mod metrics;
pub(crate) mod network;
pub(crate) mod rest_server;
pub(crate) mod shutdown_trigger;
pub mod storage;
pub(crate) mod sync_leaper;
pub(crate) mod transaction_acceptor;
pub(crate) mod upgrade_watcher;

use datasize::DataSize;
use serde::Deserialize;
use std::fmt::{Debug, Display};
use tracing::info;

use crate::{
    effect::{EffectBuilder, Effects},
    failpoints::FailpointActivation,
    NodeRng,
};

#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
///     style End fill:#66ccff,stroke:#333,stroke-width:4px
///
///     Start --> Uninitialized
///     Uninitialized --> Initializing
///     Initializing --> Initialized
///     Initializing --> Fatal
///     Initialized --> End
///     Fatal --> End
/// ```
#[derive(Clone, PartialEq, Eq, DataSize, Debug, Deserialize, Default)]
pub(crate) enum ComponentState {
    #[default]
    Uninitialized,
    Initializing,
    Initialized,
    Fatal(String),
}

/// Core Component.
///
/// Every component process a set of events it defines itself
/// Its inputs are `Event`s, allowing it to perform work whenever an event is received, outputting
/// `Effect`s each time it is called.
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
pub(crate) trait Component<REv> {
    /// Event associated with `Component`.
    ///
    /// The event type that is handled by the component.
    type Event;

    /// Name of the component.
    fn name(&self) -> &str;

    /// Activate/deactivate a failpoint.
    fn activate_failpoint(&mut self, _activation: &FailpointActivation) {
        // Default is to ignore failpoints.
    }

    /// Processes an event, outputting zero or more effects.
    ///
    /// This function must not ever perform any blocking or CPU intensive work, as it is expected
    /// to return very quickly -- it will usually be called from an `async` function context.
    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event>;
}

pub(crate) trait InitializedComponent<REv>: Component<REv> {
    fn state(&self) -> &ComponentState;

    fn is_uninitialized(&self) -> bool {
        self.state() == &ComponentState::Uninitialized
    }

    fn is_initialized(&self) -> bool {
        self.state() == &ComponentState::Initialized
    }

    fn is_fatal(&self) -> bool {
        matches!(self.state(), ComponentState::Fatal(_))
    }

    fn start_initialization(&mut self) {
        if self.is_uninitialized() {
            self.set_state(ComponentState::Initializing);
        } else {
            info!(name = self.name(), "component must be uninitialized");
        }
    }

    fn set_state(&mut self, new_state: ComponentState);
}

pub(crate) trait PortBoundComponent<REv>: InitializedComponent<REv> {
    type Error: Display + Debug;
    type ComponentEvent;

    fn bind(
        &mut self,
        enabled: bool,
        effect_builder: EffectBuilder<REv>,
    ) -> (Effects<Self::ComponentEvent>, ComponentState) {
        if !enabled {
            return (Effects::new(), ComponentState::Initialized);
        }

        match self.listen(effect_builder) {
            Ok(effects) => (effects, ComponentState::Initialized),
            Err(error) => (Effects::new(), ComponentState::Fatal(format!("{}", error))),
        }
    }

    fn listen(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error>;
}

pub(crate) trait ValidatorBoundComponent<REv>: Component<REv> {
    fn handle_validators(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
    ) -> Effects<Self::Event>;
}
