use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use derive_more::From;

use casper_storage::data_access_layer::{
    BalanceResult, BidsResult, EraValidatorsResult, QueryResult,
};
use casper_types::{BlockHash, Transfer};

use crate::{
    effect::{requests::RpcRequest, Responder},
    types::NodeId,
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
        result: QueryResult,
        main_responder: Responder<QueryResult>,
    },
    QueryEraValidatorsResult {
        result: EraValidatorsResult,
        main_responder: Responder<EraValidatorsResult>,
    },
    GetBidsResult {
        result: BidsResult,
        main_responder: Responder<BidsResult>,
    },
    GetPeersResult {
        peers: BTreeMap<NodeId, String>,
        main_responder: Responder<BTreeMap<NodeId, String>>,
    },
    GetBalanceResult {
        result: BalanceResult,
        main_responder: Responder<BalanceResult>,
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
            Event::GetPeersResult { peers, .. } => write!(formatter, "get peers: {}", peers.len()),
        }
    }
}
