use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use derive_more::From;

use casper_execution_engine::core::engine_state::{
    self, BalanceResult, GetBidsResult, GetEraValidatorsError, QueryResult,
};
use casper_types::{system::auction::EraValidators, Transfer};

use crate::{
    effect::{requests::RpcRequest, Responder},
    rpcs::chain::BlockIdentifier,
    types::{BlockHash, BlockWithMetadata, Deploy, DeployHash, DeployMetadata, NodeId},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    #[from]
    RpcRequest(RpcRequest),
    GetBlockResult {
        maybe_id: Option<BlockIdentifier>,
        result: Box<Option<BlockWithMetadata>>,
        main_responder: Responder<Option<BlockWithMetadata>>,
    },
    GetBlockTransfersResult {
        block_hash: BlockHash,
        result: Box<Option<Vec<Transfer>>>,
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
        result: Box<Option<(Deploy, DeployMetadata)>>,
        main_responder: Responder<Option<(Deploy, DeployMetadata)>>,
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
            Event::RpcRequest(request) => write!(formatter, "{}", request),
            Event::GetBlockResult {
                maybe_id: Some(BlockIdentifier::Hash(hash)),
                result,
                ..
            } => write!(formatter, "get block result for {}: {:?}", hash, result),
            Event::GetBlockResult {
                maybe_id: Some(BlockIdentifier::Height(height)),
                result,
                ..
            } => write!(formatter, "get block result for {}: {:?}", height, result),
            Event::GetBlockResult {
                maybe_id: None,
                result,
                ..
            } => write!(formatter, "get latest block result: {:?}", result),
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
