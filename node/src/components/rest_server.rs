//! API server
//!
//! The API server provides clients with two types of service: a JSON-RPC API for querying state and
//! sending commands to the node, and an event-stream returning Server-Sent Events (SSEs) holding
//! JSON-encoded data.
//!
//! The actual server is run in backgrounded tasks.   RPCs requests are translated into reactor
//! requests to various components.
//!
//! This module currently provides both halves of what is required for an API server: An abstract
//! API Server that handles API requests and an external service endpoint based on HTTP.
//!
//! For the list of supported RPCs and SSEs, see
//! https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs

mod config;
mod event;
mod filters;
mod http_server;
mod sse_server;

use std::{convert::Infallible, fmt::Debug};

use datasize::DataSize;
use futures::join;
use lazy_static::lazy_static;
use semver::Version;
use tokio::sync::mpsc::{self, UnboundedSender};

use super::Component;
use crate::{
    components::storage::Storage,
    effect::{
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, LinearChainRequest, MetricsRequest,
            NetworkInfoRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    small_network::NodeId,
    types::{CryptoRngCore, StatusFeed},
};

use crate::effect::requests::RestRequest;
pub use config::Config;
pub(crate) use event::Event;
pub use sse_server::SseData;

// TODO - confirm if we want to use the protocol version for this.
lazy_static! {
    static ref CLIENT_API_VERSION: Version = Version::new(1, 0, 0);
}

/// A helper trait whose bounds represent the requirements for a reactor event that `run_server` can
/// work with.
trait ReactorEventT:
    From<Event>
    + From<RestRequest<NodeId>>
    + From<StorageRequest<Storage>>
    + From<LinearChainRequest<NodeId>>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<RestRequest<NodeId>>
        + From<StorageRequest<Storage>>
        + From<LinearChainRequest<NodeId>>
        + From<ContractRuntimeRequest>
        + Send
        + 'static
{
}

#[derive(DataSize, Debug)]
pub(crate) struct RestServer {
    /// Channel sender to pass event-stream data to the event-stream server.
    // TODO - this should not be skipped.  Awaiting support for `UnboundedSender` in datasize crate.
    #[data_size(skip)]
    sse_data_sender: UnboundedSender<SseData>,
}

impl RestServer {
    pub(crate) fn new<REv>(config: Config, effect_builder: EffectBuilder<REv>) -> Self
    where
        REv: From<Event>
            + From<RestRequest<NodeId>>
            + From<StorageRequest<Storage>>
            + From<LinearChainRequest<NodeId>>
            + From<ContractRuntimeRequest>
            + Send,
    {
        let (sse_data_sender, sse_data_receiver) = mpsc::unbounded_channel();
        tokio::spawn(http_server::run(config, effect_builder, sse_data_receiver));

        RestServer { sse_data_sender }
    }
}

impl<REv> Component<REv> for RestServer
where
    REv: From<NetworkInfoRequest<NodeId>>
        + From<LinearChainRequest<NodeId>>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + From<MetricsRequest>
        + From<StorageRequest<Storage>>
        + From<Event>
        + From<RestRequest<NodeId>>
        + Send,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::RestRequest(RestRequest::GetStatus { responder }) => async move {
                let (last_added_block, peers, chainspec_info) = join!(
                    effect_builder.get_highest_block(),
                    effect_builder.network_peers(),
                    effect_builder.get_chainspec_info()
                );
                let status_feed = StatusFeed::new(last_added_block, peers, chainspec_info);
                responder.respond(status_feed).await;
            }
            .ignore(),
            Event::RestRequest(RestRequest::GetMetrics { responder }) => effect_builder
                .get_metrics()
                .event(move |text| Event::GetMetricsResult {
                    text,
                    main_responder: responder,
                }),
            Event::GetMetricsResult {
                text,
                main_responder,
            } => main_responder.respond(text).ignore(),
        }
    }
}
