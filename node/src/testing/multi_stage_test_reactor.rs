pub mod test_chain;

use std::{
    fmt::{self, Display, Formatter},
    mem,
    path::PathBuf,
};

use derive_more::From;
use futures::FutureExt;
use once_cell::sync::Lazy;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error;
use tracing::warn;

use crate::{
    components::{consensus::EraSupervisor, storage::Storage},
    effect::{EffectBuilder, EffectExt, Effects},
    reactor::{
        initializer::Reactor as InitializerReactor,
        joiner::Reactor as JoinerReactor,
        validator::{Reactor as ValidatorReactor, ValidatorInitConfig},
        wrap_effects, EventQueueHandle, QueueKind, Reactor, Scheduler,
    },
    testing::network::NetworkedReactor,
    types::{Chainspec, NodeId},
    utils::{self, WithDir, RESOURCES_PATH},
    NodeRng,
};

pub static CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| RESOURCES_PATH.join("local"));

#[derive(Debug, Error)]
pub enum MultiStageTestReactorError {
    #[error("Could not make initializer reactor: {0}")]
    CouldNotMakeInitializerReactor(<InitializerReactor as Reactor>::Error),

    #[error(transparent)]
    PrometheusError(#[from] prometheus::Error),
}

#[derive(Debug, From, Serialize)]
pub enum MultiStageTestEvent {
    // Events wrapping internal reactor events.
    #[from]
    InitializerEvent(<InitializerReactor as Reactor>::Event),
    #[from]
    JoinerEvent(<JoinerReactor as Reactor>::Event),
    #[from]
    ValidatorEvent(<ValidatorReactor as Reactor>::Event),

    // Events related to stage transitions.
    JoinerFinalized(#[serde(skip_serializing)] Box<ValidatorInitConfig>),
}

impl Display for MultiStageTestEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MultiStageTestEvent::InitializerEvent(ev) => {
                write!(f, "initializer event: {}", ev)
            }
            MultiStageTestEvent::JoinerEvent(ev) => {
                write!(f, "joiner event: {}", ev)
            }
            MultiStageTestEvent::ValidatorEvent(ev) => {
                write!(f, "validator event: {}", ev)
            }
            MultiStageTestEvent::JoinerFinalized(_) => {
                write!(f, "joiner finalization complete")
            }
        }
    }
}

pub(crate) enum MultiStageTestReactor {
    Deactivated,
    Initializer {
        initializer_reactor: Box<InitializerReactor>,
        initializer_event_queue_handle: EventQueueHandle<<InitializerReactor as Reactor>::Event>,
        registry: Box<Registry>,
    },
    Joiner {
        joiner_reactor: Box<JoinerReactor>,
        joiner_event_queue_handle: EventQueueHandle<<JoinerReactor as Reactor>::Event>,
        registry: Box<Registry>,
    },
    JoinerFinalizing {
        maybe_validator_init_config: Option<Box<ValidatorInitConfig>>,
        node_id: NodeId,
        registry: Box<Registry>,
    },
    Validator {
        validator_reactor: Box<ValidatorReactor>,
        validator_event_queue_handle: EventQueueHandle<<ValidatorReactor as Reactor>::Event>,
    },
}

impl MultiStageTestReactor {
    pub fn consensus(&self) -> Option<&EraSupervisor<NodeId>> {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer { .. } => None,
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => {
                Some(joiner_reactor.consensus())
            }
            MultiStageTestReactor::JoinerFinalizing {
                maybe_validator_init_config: None,
                ..
            } => None,
            MultiStageTestReactor::JoinerFinalizing {
                maybe_validator_init_config: Some(validator_init_config),
                ..
            } => Some(validator_init_config.consensus()),
            MultiStageTestReactor::Validator {
                validator_reactor, ..
            } => Some(validator_reactor.consensus()),
        }
    }

    pub fn storage(&self) -> Option<&Storage> {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer {
                initializer_reactor,
                ..
            } => Some(initializer_reactor.storage()),
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => Some(joiner_reactor.storage()),
            MultiStageTestReactor::JoinerFinalizing {
                maybe_validator_init_config: None,
                ..
            } => None,
            MultiStageTestReactor::JoinerFinalizing {
                maybe_validator_init_config: Some(validator_init_config),
                ..
            } => Some(validator_init_config.storage()),
            MultiStageTestReactor::Validator {
                validator_reactor, ..
            } => Some(validator_reactor.storage()),
        }
    }
}

/// Long-running task that forwards events arriving on one scheduler to another.
async fn forward_to_queue<I, O>(source: &Scheduler<I>, target_queue: EventQueueHandle<O>)
where
    O: From<I>,
{
    // Note: This will keep waiting forever if the sending end disappears, which is fine for tests.
    loop {
        let (event, queue_kind) = source.pop().await;
        target_queue.schedule(event, queue_kind).await;
    }
}

pub(crate) struct InitializerReactorConfigWithChainspec {
    config: <InitializerReactor as Reactor>::Config,
    chainspec: Chainspec,
}

impl Reactor for MultiStageTestReactor {
    type Event = MultiStageTestEvent;

    type Config = InitializerReactorConfigWithChainspec;

    type Error = MultiStageTestReactorError;

    fn new(
        initializer_reactor_config_with_chainspec: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let initializer_scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
        let initializer_event_queue_handle: EventQueueHandle<
            <InitializerReactor as Reactor>::Event,
        > = EventQueueHandle::new(initializer_scheduler);

        tokio::spawn(forward_to_queue(initializer_scheduler, event_queue));

        let (initializer_reactor, initializer_effects) = InitializerReactor::new_with_chainspec(
            initializer_reactor_config_with_chainspec.config,
            &registry,
            initializer_event_queue_handle,
            initializer_reactor_config_with_chainspec.chainspec,
        )
        .map_err(MultiStageTestReactorError::CouldNotMakeInitializerReactor)?;

        Ok((
            MultiStageTestReactor::Initializer {
                initializer_reactor: Box::new(initializer_reactor),
                initializer_event_queue_handle,
                registry: Box::new(registry.clone()),
            },
            wrap_effects(MultiStageTestEvent::InitializerEvent, initializer_effects),
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: MultiStageTestEvent,
    ) -> Effects<Self::Event> {
        // We need to enforce node ids stay constant through state transitions
        let old_node_id = self.node_id();

        // Take ownership of self
        let mut multi_stage_test_reactor = mem::replace(self, MultiStageTestReactor::Deactivated);

        // Keep track of whether the event signalled we should do a state transition
        let mut should_transition = false;

        // Process the event
        let mut effects = match (event, &mut multi_stage_test_reactor) {
            (event, MultiStageTestReactor::Deactivated) => {
                unreachable!(
                    "Event sent to deactivated three stage test reactor: {}",
                    event
                )
            }
            (
                MultiStageTestEvent::InitializerEvent(initializer_event),
                MultiStageTestReactor::Initializer {
                    ref mut initializer_reactor,
                    initializer_event_queue_handle,
                    ..
                },
            ) => {
                let effect_builder = EffectBuilder::new(*initializer_event_queue_handle);

                let effects = wrap_effects(
                    MultiStageTestEvent::InitializerEvent,
                    initializer_reactor.dispatch_event(effect_builder, rng, initializer_event),
                );

                if initializer_reactor.is_stopped() {
                    if !initializer_reactor.stopped_successfully() {
                        panic!("failed to transition from initializer to joiner");
                    }

                    should_transition = true;
                }

                effects
            }
            (
                MultiStageTestEvent::JoinerEvent(joiner_event),
                MultiStageTestReactor::Joiner {
                    ref mut joiner_reactor,
                    joiner_event_queue_handle,
                    ..
                },
            ) => {
                let effect_builder = EffectBuilder::new(*joiner_event_queue_handle);

                let effects = wrap_effects(
                    MultiStageTestEvent::JoinerEvent,
                    joiner_reactor.dispatch_event(effect_builder, rng, joiner_event),
                );

                if joiner_reactor.is_stopped() {
                    should_transition = true;
                }

                effects
            }
            (
                MultiStageTestEvent::JoinerFinalized(validator_config),
                MultiStageTestReactor::JoinerFinalizing {
                    ref mut maybe_validator_init_config,
                    ..
                },
            ) => {
                should_transition = true;

                *maybe_validator_init_config = Some(validator_config);

                // No effects, just transitioning.
                Effects::new()
            }
            (
                MultiStageTestEvent::ValidatorEvent(validator_event),
                MultiStageTestReactor::Validator {
                    ref mut validator_reactor,
                    validator_event_queue_handle,
                    ..
                },
            ) => {
                let effect_builder = EffectBuilder::new(*validator_event_queue_handle);

                let effects = wrap_effects(
                    MultiStageTestEvent::ValidatorEvent,
                    validator_reactor.dispatch_event(effect_builder, rng, validator_event),
                );

                if validator_reactor.is_stopped() {
                    panic!("validator reactor should never stop");
                }

                effects
            }
            (event, three_stage_test_reactor) => {
                let stage = match three_stage_test_reactor {
                    MultiStageTestReactor::Deactivated => "Deactivated",
                    MultiStageTestReactor::Initializer { .. } => "Initializing",
                    MultiStageTestReactor::Joiner { .. } => "Joining",
                    MultiStageTestReactor::JoinerFinalizing { .. } => "Finalizing joiner",
                    MultiStageTestReactor::Validator { .. } => "Validating",
                };

                warn!(
                    ?event,
                    ?stage,
                    "discarded event due to not being in the right stage"
                );

                // Shouldn't be reachable
                Effects::new()
            }
        };

        if should_transition {
            match multi_stage_test_reactor {
                MultiStageTestReactor::Deactivated => {
                    // We will never transition when `Deactivated`
                    unreachable!()
                }
                MultiStageTestReactor::Initializer {
                    initializer_reactor,
                    initializer_event_queue_handle,
                    registry,
                } => {
                    let dropped_events_count = effects.len();
                    if dropped_events_count != 0 {
                        warn!("when transitioning from initializer to joiner, left {} effects unhandled", dropped_events_count)
                    }

                    assert_eq!(
                        initializer_event_queue_handle
                            .event_queues_counts()
                            .values()
                            .sum::<usize>(),
                        0,
                        "before transitioning from joiner to validator, \
                         there should be no unprocessed events"
                    );

                    let joiner_scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
                    let joiner_event_queue_handle = EventQueueHandle::new(joiner_scheduler);

                    tokio::spawn(forward_to_queue(
                        joiner_scheduler,
                        effect_builder.into_inner(),
                    ));

                    let (joiner_reactor, joiner_effects) = JoinerReactor::new(
                        WithDir::new(&*CONFIG_DIR, *initializer_reactor),
                        &registry,
                        joiner_event_queue_handle,
                        rng,
                    )
                    .expect("joiner initialization failed");

                    *self = MultiStageTestReactor::Joiner {
                        joiner_reactor: Box::new(joiner_reactor),
                        joiner_event_queue_handle,
                        registry,
                    };

                    effects.extend(
                        wrap_effects(MultiStageTestEvent::JoinerEvent, joiner_effects).into_iter(),
                    )
                }
                MultiStageTestReactor::Joiner {
                    joiner_reactor,
                    joiner_event_queue_handle,
                    registry,
                } => {
                    let dropped_events_count = effects.len();
                    if dropped_events_count != 0 {
                        warn!("when transitioning from joiner to validator, left {} effects unhandled", dropped_events_count)
                    }

                    assert_eq!(
                        joiner_event_queue_handle
                            .event_queues_counts()
                            .values()
                            .sum::<usize>(),
                        0,
                        "before transitioning from joiner to validator, \
                         there should be no unprocessed events"
                    );

                    // `into_validator_config` is just waiting for networking sockets to shut down
                    // and will not stall on disabled event processing, so it is safe to block here.
                    // Since shutting down the joiner is an `async` function, we offload it into an
                    // effect and let runner do it.

                    let node_id = joiner_reactor.node_id();
                    effects.extend(joiner_reactor.into_validator_config().boxed().event(
                        |validator_init_config| {
                            MultiStageTestEvent::JoinerFinalized(Box::new(validator_init_config))
                        },
                    ));

                    *self = MultiStageTestReactor::JoinerFinalizing {
                        maybe_validator_init_config: None,
                        node_id,
                        registry,
                    };
                }
                MultiStageTestReactor::JoinerFinalizing {
                    maybe_validator_init_config: opt_validator_config,
                    node_id: _,
                    registry,
                } => {
                    let validator_config = opt_validator_config.expect("trying to transition from joiner finalizing into validator, but there is no validator config?");

                    // JoinerFinalizing transitions into a validator.
                    let validator_scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
                    let validator_event_queue_handle = EventQueueHandle::new(validator_scheduler);

                    tokio::spawn(forward_to_queue(
                        validator_scheduler,
                        effect_builder.into_inner(),
                    ));

                    let (validator_reactor, validator_effects) = ValidatorReactor::new(
                        *validator_config,
                        &registry,
                        validator_event_queue_handle,
                        rng,
                    )
                    .expect("validator intialization failed");

                    *self = MultiStageTestReactor::Validator {
                        validator_reactor: Box::new(validator_reactor),
                        validator_event_queue_handle,
                    };

                    effects.extend(
                        wrap_effects(MultiStageTestEvent::ValidatorEvent, validator_effects)
                            .into_iter(),
                    )
                }
                MultiStageTestReactor::Validator { .. } => {
                    // Validator reactors don't transition to anything
                    unreachable!()
                }
            }
        } else {
            // The reactor's state didn't transition, so put back the reactor we seized ownership of
            *self = multi_stage_test_reactor;
        }

        let new_node_id = self.node_id();
        assert_eq!(old_node_id, new_node_id);

        if let MultiStageTestReactor::Deactivated = self {
            panic!("Reactor should not longer be Deactivated!")
        }

        effects
    }
}

impl NetworkedReactor for MultiStageTestReactor {
    type NodeId = NodeId;
    fn node_id(&self) -> Self::NodeId {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer {
                initializer_reactor,
                ..
            } => initializer_reactor.node_id(),
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => joiner_reactor.node_id(),
            MultiStageTestReactor::JoinerFinalizing { node_id, .. } => node_id.clone(),
            MultiStageTestReactor::Validator {
                validator_reactor, ..
            } => validator_reactor.node_id(),
        }
    }
}
