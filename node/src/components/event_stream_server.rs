//! Event stream server
//!
//! The event stream server provides clients with an event-stream returning Server-Sent Events
//! (SSEs) holding JSON-encoded data.
//!
//! The actual server is run in backgrounded tasks.
//!
//! This module currently provides both halves of what is required for an API server:
//! a component implementation that interfaces with other components via being plugged into a
//! reactor, and an external facing http server that manages SSE subscriptions on a single endpoint.
//!
//! This component is passive and receives announcements made by other components while never making
//! a request of other components itself. The handled announcements are serialized to JSON and
//! pushed to subscribers.
//!
//! This component uses a ring buffer for outbound events providing some robustness against
//! unintended subscriber disconnects, if a disconnected subscriber re-subscribes before the buffer
//! has advanced past their last received event.

mod config;
mod event;
mod event_indexer;
mod http_server;
mod sse_server;
#[cfg(test)]
mod tests;

use std::{fmt::Debug, net::SocketAddr, path::PathBuf};

use datasize::DataSize;
use tokio::sync::{
    mpsc::{self, UnboundedSender},
    oneshot,
};
use tracing::{error, info, warn};
use warp::Filter;

use casper_types::{InitiatorAddr, ProtocolVersion};

use super::Component;
use crate::{
    components::{ComponentState, InitializedComponent, PortBoundComponent},
    effect::{EffectBuilder, Effects},
    reactor::main_reactor::MainEvent,
    types::TransactionHeader,
    utils::{self, ListeningError},
    NodeRng,
};
pub use config::Config;
pub(crate) use event::Event;
use event_indexer::{EventIndex, EventIndexer};
use sse_server::ChannelsAndFilter;
pub(crate) use sse_server::SseData;

const COMPONENT_NAME: &str = "event_stream_server";

/// This is used to define the number of events to buffer in the tokio broadcast channel to help
/// slower clients to try to avoid missing events (See
/// <https://docs.rs/tokio/1.4.0/tokio/sync/broadcast/index.html#lagging> for further details).  The
/// resulting broadcast channel size is `ADDITIONAL_PERCENT_FOR_BROADCAST_CHANNEL_SIZE` percent
/// greater than `config.event_stream_buffer_length`.
///
/// We always want the broadcast channel size to be greater than the event stream buffer length so
/// that a new client can retrieve the entire set of buffered events if desired.
const ADDITIONAL_PERCENT_FOR_BROADCAST_CHANNEL_SIZE: u32 = 20;

/// A helper trait whose bounds represent the requirements for a reactor event that `run_server` can
/// work with.
pub trait ReactorEventT: From<Event> + Send {}

impl<REv> ReactorEventT for REv where REv: From<Event> + Send + 'static {}

#[derive(DataSize, Debug)]
struct InnerServer {
    /// Channel sender to pass event-stream data to the event-stream server.
    // TODO - this should not be skipped.  Awaiting support for `UnboundedSender` in datasize crate.
    #[data_size(skip)]
    sse_data_sender: UnboundedSender<(EventIndex, SseData)>,
    event_indexer: EventIndexer,
    listening_address: SocketAddr,
}

#[derive(DataSize, Debug)]
pub(crate) struct EventStreamServer {
    state: ComponentState,
    config: Config,
    storage_path: PathBuf,
    api_version: ProtocolVersion,
    sse_server: Option<InnerServer>,
}

impl EventStreamServer {
    pub(crate) fn new(config: Config, storage_path: PathBuf, api_version: ProtocolVersion) -> Self {
        EventStreamServer {
            state: ComponentState::Uninitialized,
            config,
            storage_path,
            api_version,
            sse_server: None,
        }
    }

    fn listen(&mut self) -> Result<(), ListeningError> {
        let required_address = utils::resolve_address(&self.config.address).map_err(|error| {
            warn!(
                %error,
                address=%self.config.address,
                "failed to start event stream server, cannot parse address"
            );
            ListeningError::ResolveAddress(error)
        })?;

        // Event stream channels and filter.
        let broadcast_channel_size = self.config.event_stream_buffer_length
            * (100 + ADDITIONAL_PERCENT_FOR_BROADCAST_CHANNEL_SIZE)
            / 100;

        let ChannelsAndFilter {
            event_broadcaster,
            new_subscriber_info_receiver,
            sse_filter,
        } = ChannelsAndFilter::new(
            broadcast_channel_size as usize,
            self.config.max_concurrent_subscribers,
        );

        let (server_shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

        let (sse_data_sender, sse_data_receiver) = mpsc::unbounded_channel();

        let listening_address = match self.config.cors_origin.as_str() {
            "" => {
                let (listening_address, server_with_shutdown) = warp::serve(sse_filter)
                    .try_bind_with_graceful_shutdown(required_address, async {
                        shutdown_receiver.await.ok();
                    })
                    .map_err(|error| ListeningError::Listen {
                        address: required_address,
                        error: Box::new(error),
                    })?;

                tokio::spawn(http_server::run(
                    self.config.clone(),
                    self.api_version,
                    server_with_shutdown,
                    server_shutdown_sender,
                    sse_data_receiver,
                    event_broadcaster,
                    new_subscriber_info_receiver,
                ));
                listening_address
            }
            "*" => {
                let (listening_address, server_with_shutdown) =
                    warp::serve(sse_filter.with(warp::cors().allow_any_origin()))
                        .try_bind_with_graceful_shutdown(required_address, async {
                            shutdown_receiver.await.ok();
                        })
                        .map_err(|error| ListeningError::Listen {
                            address: required_address,
                            error: Box::new(error),
                        })?;

                tokio::spawn(http_server::run(
                    self.config.clone(),
                    self.api_version,
                    server_with_shutdown,
                    server_shutdown_sender,
                    sse_data_receiver,
                    event_broadcaster,
                    new_subscriber_info_receiver,
                ));
                listening_address
            }
            _ => {
                let (listening_address, server_with_shutdown) = warp::serve(
                    sse_filter.with(warp::cors().allow_origin(self.config.cors_origin.as_str())),
                )
                .try_bind_with_graceful_shutdown(required_address, async {
                    shutdown_receiver.await.ok();
                })
                .map_err(|error| ListeningError::Listen {
                    address: required_address,
                    error: Box::new(error),
                })?;

                tokio::spawn(http_server::run(
                    self.config.clone(),
                    self.api_version,
                    server_with_shutdown,
                    server_shutdown_sender,
                    sse_data_receiver,
                    event_broadcaster,
                    new_subscriber_info_receiver,
                ));
                listening_address
            }
        };

        info!(address=%listening_address, "started event stream server");

        let event_indexer = EventIndexer::new(self.storage_path.clone());

        self.sse_server = Some(InnerServer {
            sse_data_sender,
            event_indexer,
            listening_address,
        });
        Ok(())
    }

    /// Broadcasts the SSE data to all clients connected to the event stream.
    fn broadcast(&mut self, sse_data: SseData) -> Effects<Event> {
        if let Some(server) = self.sse_server.as_mut() {
            let event_index = server.event_indexer.next_index();
            let _ = server.sse_data_sender.send((event_index, sse_data));
        }
        Effects::new()
    }
}

impl Drop for EventStreamServer {
    fn drop(&mut self) {
        let _ = self.broadcast(SseData::Shutdown);
    }
}

impl<REv> Component<REv> for EventStreamServer
where
    REv: ReactorEventT,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => {
                    let (effects, state) = self.bind(self.config.enable_server, effect_builder);
                    <Self as InitializedComponent<MainEvent>>::set_state(self, state);
                    effects
                }
                Event::BlockAdded(_)
                | Event::TransactionAccepted(_)
                | Event::TransactionProcessed { .. }
                | Event::TransactionsExpired(_)
                | Event::Fault { .. }
                | Event::FinalitySignature(_)
                | Event::Step { .. } => {
                    warn!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "should not handle this event when component is pending initialization"
                    );
                    Effects::new()
                }
            },
            ComponentState::Initialized => match event {
                Event::Initialize => {
                    error!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::BlockAdded(block) => self.broadcast(SseData::BlockAdded {
                    block_hash: *block.hash(),
                    block: Box::new((*block).clone()),
                }),
                Event::TransactionAccepted(transaction) => {
                    self.broadcast(SseData::TransactionAccepted { transaction })
                }
                Event::TransactionProcessed {
                    transaction_hash,
                    transaction_header,
                    block_hash,
                    execution_result,
                    messages,
                } => {
                    let (initiator_addr, timestamp, ttl) = match *transaction_header {
                        TransactionHeader::Deploy(deploy_header) => (
                            InitiatorAddr::PublicKey(deploy_header.account().clone()),
                            deploy_header.timestamp(),
                            deploy_header.ttl(),
                        ),
                        TransactionHeader::V1(metadata) => (
                            metadata.initiator_addr().clone(),
                            metadata.timestamp(),
                            metadata.ttl(),
                        ),
                    };
                    self.broadcast(SseData::TransactionProcessed {
                        transaction_hash: Box::new(transaction_hash),
                        initiator_addr: Box::new(initiator_addr),
                        timestamp,
                        ttl,
                        block_hash: Box::new(block_hash),
                        execution_result,
                        messages,
                    })
                }
                Event::TransactionsExpired(transaction_hashes) => transaction_hashes
                    .into_iter()
                    .flat_map(|transaction_hash| {
                        self.broadcast(SseData::TransactionExpired { transaction_hash })
                    })
                    .collect(),
                Event::Fault {
                    era_id,
                    public_key,
                    timestamp,
                } => self.broadcast(SseData::Fault {
                    era_id,
                    public_key,
                    timestamp,
                }),
                Event::FinalitySignature(fs) => self.broadcast(SseData::FinalitySignature(fs)),
                Event::Step {
                    era_id,
                    execution_effects,
                } => self.broadcast(SseData::Step {
                    era_id,
                    execution_effects,
                }),
            },
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv> InitializedComponent<REv> for EventStreamServer
where
    REv: ReactorEventT,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
    }
}

impl<REv> PortBoundComponent<REv> for EventStreamServer
where
    REv: ReactorEventT,
{
    type Error = ListeningError;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error> {
        self.listen()?;
        Ok(Effects::new())
    }
}
