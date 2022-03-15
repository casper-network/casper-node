pub(crate) mod test_chain;

use std::{
    fmt::{self, Display, Formatter},
    mem,
    path::PathBuf,
    sync::Arc,
};

use derive_more::From;
use futures::FutureExt;
use once_cell::sync::Lazy;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error;
use tracing::warn;

use crate::{
    components::{consensus::EraSupervisor, contract_runtime::ContractRuntime, storage::Storage},
    effect::{announcements::ControlAnnouncement, EffectBuilder, EffectExt, Effects},
    reactor::{
        initializer::Reactor as InitializerReactor,
        joiner::Reactor as JoinerReactor,
        participating::{ParticipatingInitConfig, Reactor as ParticipatingReactor},
        wrap_effects, EventQueueHandle, QueueKind, Reactor, ReactorEvent, ReactorExit, Scheduler,
    },
    testing::network::NetworkedReactor,
    types::{Chainspec, ChainspecRawBytes, NodeId},
    utils::{self, WithDir, RESOURCES_PATH},
    NodeRng,
};

pub(crate) static CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| RESOURCES_PATH.join("local"));

#[derive(Debug, Error)]
pub(crate) enum MultiStageTestReactorError {
    #[error("Could not make initializer reactor: {0}")]
    CouldNotMakeInitializerReactor(<InitializerReactor as Reactor>::Error),

    #[error(transparent)]
    PrometheusError(#[from] prometheus::Error),
}

#[derive(Debug, From, Serialize)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum MultiStageTestEvent {
    // Events wrapping internal reactor events.
    #[from]
    InitializerEvent(<InitializerReactor as Reactor>::Event),
    #[from]
    JoinerEvent(<JoinerReactor as Reactor>::Event),
    #[from]
    ParticipatingEvent(<ParticipatingReactor as Reactor>::Event),

    // Events related to stage transitions.
    JoinerFinalized(#[serde(skip_serializing)] Box<ParticipatingInitConfig>),

    // Control announcement.
    // These would only be used for fatal errors emitted by the multi-stage reactor itself, all
    // "real" control announcements will be inside `InitializerEvent`, `JoinerEvent` or
    // `ParticipatingEvent`.
    #[from]
    ControlAnnouncement(ControlAnnouncement),
}

impl ReactorEvent for MultiStageTestEvent {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
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
            MultiStageTestEvent::ParticipatingEvent(ev) => {
                write!(f, "participating event: {}", ev)
            }
            MultiStageTestEvent::JoinerFinalized(_) => {
                write!(f, "joiner finalization complete")
            }
            MultiStageTestEvent::ControlAnnouncement(ctrl_ann) => {
                write!(f, "control: {}", ctrl_ann)
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
        maybe_participating_init_config: Option<Box<ParticipatingInitConfig>>,
        node_id: NodeId,
        registry: Box<Registry>,
    },
    Participating {
        participating_reactor: Box<ParticipatingReactor>,
        participating_event_queue_handle:
            EventQueueHandle<<ParticipatingReactor as Reactor>::Event>,
    },
}

impl MultiStageTestReactor {
    pub(crate) fn consensus(&self) -> Option<&EraSupervisor> {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer { .. }
            | MultiStageTestReactor::Joiner { .. }
            | MultiStageTestReactor::JoinerFinalizing { .. } => None,
            MultiStageTestReactor::Participating {
                participating_reactor,
                ..
            } => Some(participating_reactor.consensus()),
        }
    }

    pub(crate) fn storage(&self) -> Option<&Storage> {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer {
                initializer_reactor,
                ..
            } => Some(initializer_reactor.storage()),
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => Some(joiner_reactor.storage()),
            MultiStageTestReactor::JoinerFinalizing {
                maybe_participating_init_config: None,
                ..
            } => None,
            MultiStageTestReactor::JoinerFinalizing {
                maybe_participating_init_config: Some(participating_init_config),
                ..
            } => Some(participating_init_config.storage()),
            MultiStageTestReactor::Participating {
                participating_reactor,
                ..
            } => Some(participating_reactor.storage()),
        }
    }

    pub(crate) fn contract_runtime(&self) -> Option<&ContractRuntime> {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer {
                initializer_reactor,
                ..
            } => Some(initializer_reactor.contract_runtime()),
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => {
                Some(joiner_reactor.contract_runtime())
            }
            MultiStageTestReactor::JoinerFinalizing {
                maybe_participating_init_config: None,
                ..
            } => None,
            MultiStageTestReactor::JoinerFinalizing {
                maybe_participating_init_config: Some(participating_init_config),
                ..
            } => Some(participating_init_config.contract_runtime()),
            MultiStageTestReactor::Participating {
                participating_reactor,
                ..
            } => Some(participating_reactor.contract_runtime()),
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
        let ((_ancestor, event), queue_kind) = source.pop().await;
        target_queue.schedule(event, queue_kind).await;
    }
}

pub(crate) struct InitializerReactorConfigWithChainspec {
    config: <InitializerReactor as Reactor>::Config,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,
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
        > = EventQueueHandle::without_shutdown(initializer_scheduler);

        tokio::spawn(forward_to_queue(initializer_scheduler, event_queue));

        let (initializer_reactor, initializer_effects) = InitializerReactor::new_with_chainspec(
            initializer_reactor_config_with_chainspec.config,
            registry,
            initializer_event_queue_handle,
            initializer_reactor_config_with_chainspec.chainspec,
            initializer_reactor_config_with_chainspec.chainspec_raw_bytes,
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

                match initializer_reactor.maybe_exit() {
                    Some(ReactorExit::ProcessShouldContinue) => {
                        should_transition = true;
                    }
                    Some(_) => panic!("failed to transition from initializer to joiner"),
                    None => (),
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

                match joiner_reactor.maybe_exit() {
                    Some(ReactorExit::ProcessShouldContinue) => {
                        should_transition = true;
                    }
                    Some(_) => panic!("failed to transition from initializer to joiner"),
                    None => (),
                }

                effects
            }
            (
                MultiStageTestEvent::JoinerFinalized(participating_config),
                MultiStageTestReactor::JoinerFinalizing {
                    ref mut maybe_participating_init_config,
                    ..
                },
            ) => {
                should_transition = true;

                *maybe_participating_init_config = Some(participating_config);

                // No effects, just transitioning.
                Effects::new()
            }
            (
                MultiStageTestEvent::ParticipatingEvent(participating_event),
                MultiStageTestReactor::Participating {
                    ref mut participating_reactor,
                    participating_event_queue_handle,
                    ..
                },
            ) => {
                let effect_builder = EffectBuilder::new(*participating_event_queue_handle);

                let effects = wrap_effects(
                    MultiStageTestEvent::ParticipatingEvent,
                    participating_reactor.dispatch_event(effect_builder, rng, participating_event),
                );

                if participating_reactor.maybe_exit().is_some() {
                    panic!("participating reactor should never stop");
                }

                effects
            }
            (event, three_stage_test_reactor) => {
                let stage = match three_stage_test_reactor {
                    MultiStageTestReactor::Deactivated => "Deactivated",
                    MultiStageTestReactor::Initializer { .. } => "Initializing",
                    MultiStageTestReactor::Joiner { .. } => "Joining",
                    MultiStageTestReactor::JoinerFinalizing { .. } => "Finalizing joiner",
                    MultiStageTestReactor::Participating { .. } => "Participating",
                };

                warn!(
                    ?event,
                    ?stage,
                    "discarded event due to not being in the right stage"
                );

                // TODO: Fix code to stop discarding events and change below to unreachable!()
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
                        "before transitioning from initializer to joiner, there should be no \
                         unprocessed events"
                    );

                    let joiner_scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
                    let joiner_event_queue_handle =
                        EventQueueHandle::without_shutdown(joiner_scheduler);

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
                        warn!("when transitioning from joiner to participating, left {} effects unhandled", dropped_events_count)
                    }

                    let joiner_event_queue_length = joiner_event_queue_handle
                        .event_queues_counts()
                        .values()
                        .sum::<usize>();
                    if joiner_event_queue_length != 0 {
                        warn!("before transitioning from joiner to participating, there should be no unprocessed events, left {} unprocessed events", joiner_event_queue_length)
                    }

                    // `into_participating_config` is just waiting for networking sockets to shut
                    // down and will not stall on disabled event processing, so
                    // it is safe to block here. Since shutting down the joiner
                    // is an `async` function, we offload it into an effect and
                    // let runner do it.

                    let node_id = joiner_reactor.node_id();
                    effects.extend(joiner_reactor.into_participating_config().boxed().event(
                        |res_participating_init_config| {
                            let participating_init_config = res_participating_init_config.unwrap();
                            MultiStageTestEvent::JoinerFinalized(Box::new(
                                participating_init_config,
                            ))
                        },
                    ));

                    *self = MultiStageTestReactor::JoinerFinalizing {
                        maybe_participating_init_config: None,
                        node_id,
                        registry,
                    };
                }
                MultiStageTestReactor::JoinerFinalizing {
                    maybe_participating_init_config: opt_participating_config,
                    node_id: _,
                    registry,
                } => {
                    let participating_config = opt_participating_config.expect("trying to transition from joiner finalizing into participating, but there is no participating config?");

                    // JoinerFinalizing transitions into a participating.
                    let participating_scheduler = utils::leak(Scheduler::new(QueueKind::weights()));

                    let participating_event_queue_handle =
                        EventQueueHandle::without_shutdown(participating_scheduler);

                    tokio::spawn(forward_to_queue(
                        participating_scheduler,
                        effect_builder.into_inner(),
                    ));

                    let (participating_reactor, participating_effects) = ParticipatingReactor::new(
                        *participating_config,
                        &registry,
                        participating_event_queue_handle,
                        rng,
                    )
                    .expect("participating initialization failed");

                    *self = MultiStageTestReactor::Participating {
                        participating_reactor: Box::new(participating_reactor),
                        participating_event_queue_handle,
                    };

                    effects.extend(
                        wrap_effects(
                            MultiStageTestEvent::ParticipatingEvent,
                            participating_effects,
                        )
                        .into_iter(),
                    )
                }
                MultiStageTestReactor::Participating { .. } => {
                    // Participating reactors don't transition to anything
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
            panic!("Reactor should no longer be Deactivated!")
        }

        effects
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer {
                initializer_reactor,
                ..
            } => initializer_reactor.maybe_exit(),
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => joiner_reactor.maybe_exit(),
            MultiStageTestReactor::JoinerFinalizing { .. } => None,
            MultiStageTestReactor::Participating {
                participating_reactor,
                ..
            } => participating_reactor.maybe_exit(),
        }
    }
}

impl NetworkedReactor for MultiStageTestReactor {
    fn node_id(&self) -> NodeId {
        match self {
            MultiStageTestReactor::Deactivated => unreachable!(),
            MultiStageTestReactor::Initializer {
                initializer_reactor,
                ..
            } => initializer_reactor.node_id(),
            MultiStageTestReactor::Joiner { joiner_reactor, .. } => joiner_reactor.node_id(),
            MultiStageTestReactor::JoinerFinalizing { node_id, .. } => *node_id,
            MultiStageTestReactor::Participating {
                participating_reactor,
                ..
            } => participating_reactor.node_id(),
        }
    }
}
