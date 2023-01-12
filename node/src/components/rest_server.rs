//! REST server
//!
//! The REST server provides clients with a simple RESTful HTTP API. This component is (currently)
//! intended for basic informational / GET endpoints only; more complex operations should be handled
//! via the RPC server.
//!
//! The actual server is run in backgrounded tasks. HTTP requests are translated into reactor
//! requests to various components.
//!
//! This module currently provides both halves of what is required for an API server:
//! a component implementation that interfaces with other components via being plugged into a
//! reactor, and an external facing http server that exposes various uri routes and converts
//! HTTP requests into the appropriate component events.
//!
//! Currently this component supports two endpoints, each of which takes no arguments:
//! /status : a human readable JSON equivalent of the info-get-status rpc method.
//!     example: curl -X GET 'http://<ip>:8888/status'
//! /metrics : time series data collected from the internals of the node being queried.
//!     example: curl -X GET 'http://<ip>:8888/metrics'

mod config;
mod event;
mod filters;
mod http_server;

use std::{fmt::Debug, time::Instant};

use datasize::DataSize;
use futures::{future::BoxFuture, join, FutureExt};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{debug, error, info, warn};

use casper_types::ProtocolVersion;

use super::Component;
use crate::{
    components::{
        rpc_server::rpcs::docs::OPEN_RPC_SCHEMA, ComponentState, InitializedComponent,
        PortBoundComponent,
    },
    effect::{
        requests::{
            BlockSynchronizerRequest, ChainspecRawBytesRequest, ConsensusRequest, MetricsRequest,
            NetworkInfoRequest, ReactorStatusRequest, RestRequest, StorageRequest,
            UpgradeWatcherRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::{main_reactor::MainEvent, Finalize},
    types::{ChainspecInfo, StatusFeed},
    utils::{self, ListeningError},
    NodeRng,
};
pub use config::Config;
pub(crate) use event::Event;

const COMPONENT_NAME: &str = "rest_server";

/// A helper trait capturing all of this components Request type dependencies.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<RestRequest>
    + From<NetworkInfoRequest>
    + From<StorageRequest>
    + From<ChainspecRawBytesRequest>
    + From<UpgradeWatcherRequest>
    + From<ConsensusRequest>
    + From<MetricsRequest>
    + From<ReactorStatusRequest>
    + From<BlockSynchronizerRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<RestRequest>
        + From<NetworkInfoRequest>
        + From<StorageRequest>
        + From<ChainspecRawBytesRequest>
        + From<UpgradeWatcherRequest>
        + From<ConsensusRequest>
        + From<MetricsRequest>
        + From<ReactorStatusRequest>
        + From<BlockSynchronizerRequest>
        + Send
        + 'static
{
}

#[derive(DataSize, Debug)]
pub(crate) struct InnerRestServer {
    /// When the message is sent, it signals the server loop to exit cleanly.
    #[data_size(skip)]
    shutdown_sender: oneshot::Sender<()>,
    /// The task handle which will only join once the server loop has exited.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,
    /// The instant at which the node has started.
    node_startup_instant: Instant,
    /// The network name, as specified in the chainspec
    network_name: String,
}

#[derive(DataSize, Debug)]
pub(crate) struct RestServer {
    /// The component state.
    state: ComponentState,
    config: Config,
    api_version: ProtocolVersion,
    network_name: String,
    node_startup_instant: Instant,
    /// Inner server is present only when enabled in the config.
    inner_rest: Option<InnerRestServer>,
}

impl RestServer {
    pub(crate) fn new(
        config: Config,
        api_version: ProtocolVersion,
        network_name: String,
        node_startup_instant: Instant,
    ) -> Self {
        RestServer {
            state: ComponentState::Uninitialized,
            config,
            api_version,
            network_name,
            node_startup_instant,
            inner_rest: None,
        }
    }
}

impl<REv> Component<REv> for RestServer
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
                Event::RestRequest(_) | Event::GetMetricsResult { .. } => {
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
                Event::RestRequest(RestRequest::Status { responder }) => {
                    let node_uptime = self.node_startup_instant.elapsed();
                    let network_name = self.network_name.clone();
                    async move {
                        let (
                            last_added_block,
                            peers,
                            next_upgrade,
                            consensus_status,
                            (reactor_state, last_progress),
                            available_block_range,
                            block_sync,
                        ) = join!(
                            effect_builder.get_highest_complete_block_from_storage(),
                            effect_builder.network_peers(),
                            effect_builder.get_next_upgrade(),
                            effect_builder.consensus_status(),
                            effect_builder.get_reactor_status(),
                            effect_builder.get_available_block_range_from_storage(),
                            effect_builder.get_block_synchronizer_status(),
                        );
                        let starting_state_root_hash = effect_builder
                            .get_block_header_at_height_from_storage(
                                available_block_range.low(),
                                true,
                            )
                            .await
                            .map(|header| *header.state_root_hash());
                        let status_feed = StatusFeed::new(
                            last_added_block,
                            peers,
                            ChainspecInfo::new(network_name, next_upgrade),
                            consensus_status,
                            node_uptime,
                            reactor_state,
                            last_progress,
                            available_block_range,
                            block_sync,
                            starting_state_root_hash,
                        );
                        responder.respond(status_feed).await;
                    }
                }
                .ignore(),
                Event::RestRequest(RestRequest::Metrics { responder }) => effect_builder
                    .get_metrics()
                    .event(move |text| Event::GetMetricsResult {
                        text,
                        main_responder: responder,
                    }),
                Event::RestRequest(RestRequest::RpcSchema { responder }) => {
                    let schema = OPEN_RPC_SCHEMA.clone();
                    responder.respond(schema).ignore()
                }
                Event::GetMetricsResult {
                    text,
                    main_responder,
                } => main_responder.respond(text).ignore(),
            },
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv> InitializedComponent<REv> for RestServer
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

impl<REv> PortBoundComponent<REv> for RestServer
where
    REv: ReactorEventT,
{
    type Error = ListeningError;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error> {
        let cfg = &self.config;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

        let builder = utils::start_listening(&cfg.address)?;
        let server_join_handle = Some(tokio::spawn(http_server::run(
            builder,
            effect_builder,
            self.api_version,
            shutdown_receiver,
            cfg.qps_limit,
        )));

        let node_startup_instant = self.node_startup_instant;
        let network_name = self.network_name.clone();
        self.inner_rest = Some(InnerRestServer {
            shutdown_sender,
            server_join_handle,
            node_startup_instant,
            network_name,
        });

        Ok(Effects::new())
    }
}

impl Finalize for RestServer {
    fn finalize(self) -> BoxFuture<'static, ()> {
        async {
            if let Some(mut rest_server) = self.inner_rest {
                let _ = rest_server.shutdown_sender.send(());

                // Wait for the server to exit cleanly.
                if let Some(join_handle) = rest_server.server_join_handle.take() {
                    match join_handle.await {
                        Ok(_) => debug!("rest server exited cleanly"),
                        Err(error) => error!(%error, "could not join rest server task cleanly"),
                    }
                } else {
                    warn!("rest server shutdown while already shut down")
                }
            } else {
                info!("rest server was disabled in config, no shutdown performed")
            }
        }
        .boxed()
    }
}

#[cfg(test)]
mod schema_tests {
    use crate::{
        rpcs::{
            docs::OpenRpcSchema,
            info::{GetChainspecResult, GetValidatorChangesResult},
        },
        testing::assert_schema,
        types::GetStatusResult,
    };
    use schemars::schema_for;

    #[test]
    fn schema_status() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_status.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(schema_path, schema_for!(GetStatusResult));
    }

    #[test]
    fn schema_validator_changes() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_validator_changes.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(schema_path, schema_for!(GetValidatorChangesResult));
    }

    #[test]
    fn schema_rpc_schema() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_rpc_schema.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(schema_path, schema_for!(OpenRpcSchema));
    }

    #[test]
    fn schema_chainspec_bytes() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_chainspec_bytes.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(schema_path, schema_for!(GetChainspecResult));
    }
}
