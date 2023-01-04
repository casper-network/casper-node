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
//!
//! For details about the SSE model and a list of supported SSEs, see:
//! <https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs>

mod config;
mod event;
mod event_indexer;
mod http_server;
mod sse_server;
#[cfg(test)]
mod tests;

use std::{fmt::Debug, net::SocketAddr, path::PathBuf, sync::Arc};

use datasize::DataSize;
use tokio::sync::{
    mpsc::{self, UnboundedSender},
    oneshot,
};
use tracing::{error, info, warn};
use warp::Filter;

use casper_types::ProtocolVersion;

use super::Component;
use crate::{
    components::{ComponentState, InitializedComponent, PortBoundComponent},
    effect::{EffectBuilder, Effects},
    reactor::main_reactor::MainEvent,
    types::JsonBlock,
    utils::{self, ListeningError},
    NodeRng,
};
pub use config::Config;
pub(crate) use event::Event;
use event_indexer::{EventIndex, EventIndexer};
use sse_server::ChannelsAndFilter;
pub(crate) use sse_server::SseData;

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
    status: ComponentState,
    config: Config,
    storage_path: PathBuf,
    api_version: ProtocolVersion,
    sse_server: Option<InnerServer>,
}

impl EventStreamServer {
    pub(crate) fn new(config: Config, storage_path: PathBuf, api_version: ProtocolVersion) -> Self {
        EventStreamServer {
            status: ComponentState::Uninitialized,
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

        let (listening_address, server_with_shutdown) =
            warp::serve(sse_filter.with(warp::cors().allow_any_origin()))
                .try_bind_with_graceful_shutdown(required_address, async {
                    shutdown_receiver.await.ok();
                })
                .map_err(|error| ListeningError::Listen {
                    address: required_address,
                    error: Box::new(error),
                })?;

        info!(address=%listening_address, "started event stream server");

        let (sse_data_sender, sse_data_receiver) = mpsc::unbounded_channel();

        tokio::spawn(http_server::run(
            self.config.clone(),
            self.api_version,
            server_with_shutdown,
            server_shutdown_sender,
            sse_data_receiver,
            event_broadcaster,
            new_subscriber_info_receiver,
        ));

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
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.status {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as InitializedComponent<MainEvent>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as InitializedComponent<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => {
                    let (effects, status) = self.bind(self.config.enable_server, _effect_builder);
                    self.status = status;
                    effects
                }
                Event::BlockAdded(_)
                | Event::DeployAccepted(_)
                | Event::DeployProcessed { .. }
                | Event::DeploysExpired(_)
                | Event::Fault { .. }
                | Event::FinalitySignature(_)
                | Event::Step { .. } => {
                    warn!(
                        ?event,
                        name = <Self as InitializedComponent<MainEvent>>::name(self),
                        "should not handle this event when component is pending initialization"
                    );
                    Effects::new()
                }
            },
            ComponentState::Initialized => match event {
                Event::Initialize => {
                    error!(
                        ?event,
                        name = <Self as InitializedComponent<MainEvent>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::BlockAdded(block) => self.broadcast(SseData::BlockAdded {
                    block_hash: *block.hash(),
                    block: Box::new(JsonBlock::new(*block, None)),
                }),
                Event::DeployAccepted(deploy) => self.broadcast(SseData::DeployAccepted {
                    deploy: Arc::new(*deploy),
                }),
                Event::DeployProcessed {
                    deploy_hash,
                    deploy_header,
                    block_hash,
                    execution_result,
                } => self.broadcast(SseData::DeployProcessed {
                    deploy_hash: Box::new(deploy_hash),
                    account: Box::new(deploy_header.account().clone()),
                    timestamp: deploy_header.timestamp(),
                    ttl: deploy_header.ttl(),
                    dependencies: deploy_header.dependencies().clone(),
                    block_hash: Box::new(block_hash),
                    execution_result,
                }),
                Event::DeploysExpired(deploy_hashes) => deploy_hashes
                    .into_iter()
                    .flat_map(|deploy_hash| self.broadcast(SseData::DeployExpired { deploy_hash }))
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
                    execution_effect,
                } => self.broadcast(SseData::Step {
                    era_id,
                    execution_effect,
                }),
            },
        }
    }
}

impl<REv> InitializedComponent<REv> for EventStreamServer
where
    REv: ReactorEventT,
{
    fn status(&self) -> &ComponentState {
        &self.status
    }

    fn name(&self) -> &str {
        "event_stream_server"
    }

    fn set_status(&mut self, new_status: ComponentState) {
        self.status = new_status;
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
