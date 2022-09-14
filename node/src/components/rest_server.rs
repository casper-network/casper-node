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

use std::{convert::Infallible, fmt::Debug, time::Instant};

use datasize::DataSize;
use futures::{future::BoxFuture, join, FutureExt};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{debug, error, info, warn};

use casper_types::ProtocolVersion;

use super::Component;
use crate::{
    components::rpc_server::rpcs::docs::OPEN_RPC_SCHEMA,
    effect::{
        requests::{
            ChainspecRawBytesRequest, ConsensusRequest, MetricsRequest, NetworkInfoRequest,
            NodeStateRequest, RestRequest, StorageRequest, UpgradeWatcherRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::Finalize,
    types::{ChainspecInfo, StatusFeed},
    utils::{self, ListeningError},
    NodeRng,
};
pub use config::Config;
pub(crate) use event::Event;

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
    + From<NodeStateRequest>
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
        + From<NodeStateRequest>
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
    /// Inner server is present only when enabled in the config.
    inner_rest: Option<InnerRestServer>,
}

impl RestServer {
    pub(crate) fn new<REv>(
        config: Config,
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        network_name: String,
        node_startup_instant: Instant,
    ) -> Result<Self, ListeningError>
    where
        REv: ReactorEventT,
    {
        if !config.enable_server {
            return Ok(RestServer { inner_rest: None });
        }

        let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

        let builder = utils::start_listening(&config.address)?;
        let server_join_handle = Some(tokio::spawn(http_server::run(
            builder,
            effect_builder,
            api_version,
            shutdown_receiver,
            config.qps_limit,
        )));

        Ok(RestServer {
            inner_rest: Some(InnerRestServer {
                shutdown_sender,
                server_join_handle,
                node_startup_instant,
                network_name,
            }),
        })
    }
}

impl<REv> Component<REv> for RestServer
where
    REv: ReactorEventT,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let rest_server = match &self.inner_rest {
            Some(rest_server) => rest_server,
            None => {
                return Effects::new();
            }
        };

        match event {
            Event::RestRequest(RestRequest::Status { responder }) => {
                let node_uptime = rest_server.node_startup_instant.elapsed();
                let network_name = rest_server.network_name.clone();
                async move {
                    let (
                        last_added_block,
                        peers,
                        next_upgrade,
                        consensus_status,
                        node_state,
                    ) = join!(
                        effect_builder.get_highest_block_from_storage(),
                        effect_builder.network_peers(),
                        effect_builder.get_next_upgrade(),
                        effect_builder.consensus_status(),
                        effect_builder.get_node_state()
                    );

                    let status_feed = StatusFeed::new(
                        last_added_block,
                        peers,
                        ChainspecInfo::new(network_name, next_upgrade),
                        consensus_status,
                        node_uptime,
                        node_state,
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
        }
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
