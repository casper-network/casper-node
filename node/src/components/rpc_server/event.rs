use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use derive_more::From;

use casper_execution_engine::core::engine_state::{
    self, BalanceResult, GetBidsResult, GetEraValidatorsError, QueryResult,
};
use casper_types::{system::auction::EraValidators, BlockHash, Deploy, DeployHash, Transfer};

use crate::{
    effect::{requests::RpcRequest, Responder},
    types::{DeployMetadataExt, NodeId},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    Initialize,
    #[from]
    RpcRequest(RpcRequest),
    GetBlockTransfersResult {
        block_hash: BlockHash,
        result: Option<Vec<Transfer>>,
        main_responder: Responder<Option<Vec<Transfer>>>,
    },
    QueryGlobalStateResult {
        result: Result<QueryResult, engine_state::Error>,
        main_responder: Responder<Result<QueryResult, engine_state::Error>>,
    },
    QueryEraValidatorsResult {
        result: Result<EraValidators, GetEraValidatorsError>,
        main_responder: Responder<Result<EraValidators, GetEraValidatorsError>>,
    },
    GetBidsResult {
        result: Result<GetBidsResult, engine_state::Error>,
        main_responder: Responder<Result<GetBidsResult, engine_state::Error>>,
    },
    GetDeployResult {
        hash: DeployHash,
        result: Option<Box<(Deploy, DeployMetadataExt)>>,
        main_responder: Responder<Option<Box<(Deploy, DeployMetadataExt)>>>,
    },
    GetPeersResult {
        peers: BTreeMap<NodeId, String>,
        main_responder: Responder<BTreeMap<NodeId, String>>,
    },
    GetBalanceResult {
        result: Result<BalanceResult, engine_state::Error>,
        main_responder: Responder<Result<BalanceResult, engine_state::Error>>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::Initialize => write!(formatter, "initialize"),
            Event::RpcRequest(request) => write!(formatter, "{}", request),
            Event::GetBlockTransfersResult {
                block_hash, result, ..
            } => write!(
                formatter,
                "get block transfers result for block_hash {}: {:?}",
                block_hash, result
            ),
            Event::QueryGlobalStateResult { result, .. } => {
                write!(formatter, "query result: {:?}", result)
            }
            Event::QueryEraValidatorsResult { result, .. } => {
                write!(formatter, "query era validators result: {:?}", result)
            }
            Event::GetBidsResult { result, .. } => {
                write!(formatter, "get bids result: {:?}", result)
            }
            Event::GetBalanceResult { result, .. } => {
                write!(formatter, "balance result: {:?}", result)
            }
            Event::GetDeployResult { hash, result, .. } => {
                write!(formatter, "get deploy result for {}: {:?}", hash, result)
            }
            Event::GetPeersResult { peers, .. } => write!(formatter, "get peers: {}", peers.len()),
        }
    }
}
