use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

use derive_more::From;

use casper_execution_engine::core::engine_state::{self, QueryResult, BalanceResult};

use crate::{
    components::{small_network::NodeId, storage::DeployMetadata},
    effect::{requests::ApiRequest, Responder},
    types::{Block, BlockHash, Deploy, DeployHash},
};

#[derive(Debug, From)]
pub enum Event {
    #[from]
    ApiRequest(ApiRequest<NodeId>),
    GetBlockResult {
        maybe_hash: Option<BlockHash>,
        result: Box<Option<Block>>,
        main_responder: Responder<Option<Block>>,
    },
    QueryGlobalStateResult {
        result: Result<QueryResult, engine_state::Error>,
        main_responder: Responder<Result<QueryResult, engine_state::Error>>,
    },
    GetDeployResult {
        hash: DeployHash,
        result: Box<Option<(Deploy, DeployMetadata<Block>)>>,
        main_responder: Responder<Option<(Deploy, DeployMetadata<Block>)>>,
    },
    GetPeersResult {
        peers: HashMap<NodeId, SocketAddr>,
        main_responder: Responder<HashMap<NodeId, SocketAddr>>,
    },
    GetMetricsResult {
        text: Option<String>,
        main_responder: Responder<Option<String>>,
    },
    GetBalanceResult {
        result: Result<BalanceResult, engine_state::Error>,
        main_responder: Responder<Result<BalanceResult, engine_state::Error>>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::ApiRequest(request) => write!(formatter, "{}", request),
            Event::GetBlockResult {
                maybe_hash: Some(hash),
                result,
                ..
            } => write!(formatter, "get block result for {}: {:?}", hash, result),
            Event::GetBlockResult {
                maybe_hash: None,
                result,
                ..
            } => write!(formatter, "get latest block result: {:?}", result),
            Event::QueryGlobalStateResult { result, .. } => {
                write!(formatter, "query result: {:?}", result)
            }
            Event::GetBalanceResult { result, .. } => {
                write!(formatter, "balance result: {:?}", result)
            }
            Event::GetDeployResult { hash, result, .. } => {
                write!(formatter, "get deploy result for {}: {:?}", hash, result)
            }
            Event::GetPeersResult { peers, .. } => write!(formatter, "get peers: {}", peers.len()),
            Event::GetMetricsResult { text, .. } => match text {
                Some(txt) => write!(formatter, "get metrics ({} bytes)", txt.len()),
                None => write!(formatter, "get metrics (failed)"),
            },
        }
    }
}
