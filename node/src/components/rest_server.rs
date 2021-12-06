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
use tracing::{debug, error, warn};

use casper_types::ProtocolVersion;

use super::Component;
use crate::{
    effect::{
        requests::{
            ChainspecLoaderRequest, ConsensusRequest, MetricsRequest, NetworkInfoRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::Finalize,
    types::{NodeId, StatusFeed},
    utils::{self, ListeningError},
    NodeRng,
};

use crate::{components::rpc_server::rpcs::docs::OPEN_RPC_SCHEMA, effect::requests::RestRequest};
pub use config::Config;
pub(crate) use event::Event;

/// A helper trait capturing all of this components Request type dependencies.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<RestRequest<NodeId>>
    + From<NetworkInfoRequest<NodeId>>
    + From<StorageRequest>
    + From<ChainspecLoaderRequest>
    + From<ConsensusRequest>
    + From<MetricsRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<RestRequest<NodeId>>
        + From<NetworkInfoRequest<NodeId>>
        + From<StorageRequest>
        + From<ChainspecLoaderRequest>
        + From<ConsensusRequest>
        + From<MetricsRequest>
        + Send
        + 'static
{
}

#[derive(DataSize, Debug)]
pub(crate) struct RestServer {
    /// When the message is sent, it signals the server loop to exit cleanly.
    #[data_size(skip)]
    shutdown_sender: oneshot::Sender<()>,
    /// The task handle which will only join once the server loop has exited.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,
    /// The instant at which the node has started.
    node_startup_instant: Instant,
}

impl RestServer {
    pub(crate) fn new<REv>(
        config: Config,
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        node_startup_instant: Instant,
    ) -> Result<Self, ListeningError>
    where
        REv: ReactorEventT,
    {
        let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();

        let builder = utils::start_listening(&config.address)?;
        let server_join_handle = tokio::spawn(http_server::run(
            builder,
            effect_builder,
            api_version,
            shutdown_receiver,
            config.qps_limit,
        ));

        Ok(RestServer {
            shutdown_sender,
            server_join_handle: Some(server_join_handle),
            node_startup_instant,
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
        match event {
            Event::RestRequest(RestRequest::Status { responder }) => {
                let node_uptime = self.node_startup_instant.elapsed();
                async move {
                    let (last_added_block, peers, chainspec_info, consensus_status) = join!(
                        effect_builder.get_highest_block_from_storage(),
                        effect_builder.network_peers(),
                        effect_builder.get_chainspec_info(),
                        effect_builder.consensus_status()
                    );
                    let status_feed = StatusFeed::new(
                        last_added_block,
                        peers,
                        chainspec_info,
                        consensus_status,
                        node_uptime,
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
    fn finalize(mut self) -> BoxFuture<'static, ()> {
        async {
            let _ = self.shutdown_sender.send(());

            // Wait for the server to exit cleanly.
            if let Some(join_handle) = self.server_join_handle.take() {
                match join_handle.await {
                    Ok(_) => debug!("rest server exited cleanly"),
                    Err(error) => error!(%error, "could not join rest server task cleanly"),
                }
            } else {
                warn!("rest server shutdown while already shut down")
            }
        }
        .boxed()
    }
}

#[cfg(test)]
mod schema_tests {
    use std::{fs, env};
    use std::io::Write;

    use assert_json_diff::{assert_json_eq, assert_json_matches_no_panic, Config, CompareMode};
    use schemars::{schema_for, JsonSchema};
    use serde_json::Value;

    use crate::{
        rpcs::{docs::OpenRpcSchema, info::GetValidatorChangesResult},
        types::GetStatusResult,
    };

    fn assert_schema<T: JsonSchema>(schema_path: String) {
        let expected_schema = fs::read_to_string(&schema_path).unwrap();
        let expected_schema: Value = serde_json::from_str(&expected_schema).unwrap();

        let actual_schema = schema_for!(T);
        let actual_schema = serde_json::to_string_pretty(&actual_schema).unwrap();
        let mut temp_file = tempfile::Builder::new().suffix(".json").tempfile_in(env!("OUT_DIR")).unwrap();
        temp_file.write_all(actual_schema.as_bytes()).unwrap();
        let actual_schema: Value = serde_json::from_str(&actual_schema).unwrap();
        let (_file, path) = temp_file.keep().unwrap();
        let temp_file_path = path.as_path().clone();

        let result = assert_json_matches_no_panic(&actual_schema, &expected_schema,  Config::new(CompareMode::Strict));
        assert_eq!(result, Ok(()), "schema does not match:\nexpected:\n{}\nactual:\n{}\n", schema_path, temp_file_path.display());
        assert_json_eq!(actual_schema, expected_schema);
    }

    #[test]
    fn schema_status() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_status.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema::<GetStatusResult>(schema_path);
    }

    #[test]
    fn schema_validator_changes() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_validator_changes.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema::<GetValidatorChangesResult>(schema_path);
    }

    #[test]
    fn schema_rpc_schema() {
        let schema_path = format!(
            "{}/../resources/test/rest_schema_rpc_schema.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema::<OpenRpcSchema>(schema_path);
    }
}
