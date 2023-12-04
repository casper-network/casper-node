use std::sync::Arc;

use hyper::server::{conn::AddrIncoming, Builder};

use casper_json_rpc::{CorsOrigin, RequestHandlersBuilder};

use crate::{
    rpcs::info::{GetPeers, GetStatus, GetTransaction},
    NodeClient,
};

use super::rpcs::{
    account::{PutDeploy, PutTransaction},
    chain::{
        GetBlock, GetBlockTransfers, GetEraInfoBySwitchBlock, GetEraSummary, GetStateRootHash,
    },
    docs::ListRpcs,
    info::{GetChainspec, GetDeploy, GetValidatorChanges},
    state::{
        GetAccountInfo, GetAuctionInfo, GetBalance, GetDictionaryItem, GetItem, GetTrie,
        QueryBalance, QueryGlobalState,
    },
    RpcWithOptionalParams, RpcWithParams, RpcWithoutParams,
};

/// The URL path for all JSON-RPC requests.
pub const RPC_API_PATH: &str = "rpc";

pub const RPC_API_SERVER_NAME: &str = "JSON RPC";

/// Run the JSON-RPC server.
pub async fn run(
    node: Arc<dyn NodeClient>,
    builder: Builder<AddrIncoming>,
    qps_limit: u64,
    max_body_bytes: u32,
    cors_origin: String,
) {
    let mut handlers = RequestHandlersBuilder::new();
    PutDeploy::register_as_handler(node.clone(), &mut handlers);
    PutTransaction::register_as_handler(node.clone(), &mut handlers);
    GetBlock::register_as_handler(node.clone(), &mut handlers);
    GetBlockTransfers::register_as_handler(node.clone(), &mut handlers);
    GetStateRootHash::register_as_handler(node.clone(), &mut handlers);
    GetItem::register_as_handler(node.clone(), &mut handlers);
    QueryGlobalState::register_as_handler(node.clone(), &mut handlers);
    GetBalance::register_as_handler(node.clone(), &mut handlers);
    GetAccountInfo::register_as_handler(node.clone(), &mut handlers);
    GetDeploy::register_as_handler(node.clone(), &mut handlers);
    GetTransaction::register_as_handler(node.clone(), &mut handlers);
    GetPeers::register_as_handler(node.clone(), &mut handlers);
    GetStatus::register_as_handler(node.clone(), &mut handlers);
    GetEraInfoBySwitchBlock::register_as_handler(node.clone(), &mut handlers);
    GetEraSummary::register_as_handler(node.clone(), &mut handlers);
    GetAuctionInfo::register_as_handler(node.clone(), &mut handlers);
    GetTrie::register_as_handler(node.clone(), &mut handlers);
    GetValidatorChanges::register_as_handler(node.clone(), &mut handlers);
    ListRpcs::register_as_handler(node.clone(), &mut handlers);
    GetDictionaryItem::register_as_handler(node.clone(), &mut handlers);
    GetChainspec::register_as_handler(node.clone(), &mut handlers);
    QueryBalance::register_as_handler(node, &mut handlers);
    let handlers = handlers.build();

    match cors_origin.as_str() {
        "" => {
            super::rpcs::run(
                builder,
                handlers,
                qps_limit,
                max_body_bytes,
                RPC_API_PATH,
                RPC_API_SERVER_NAME,
            )
            .await
        }
        "*" => {
            super::rpcs::run_with_cors(
                builder,
                handlers,
                qps_limit,
                max_body_bytes,
                RPC_API_PATH,
                RPC_API_SERVER_NAME,
                CorsOrigin::Any,
            )
            .await
        }
        _ => {
            super::rpcs::run_with_cors(
                builder,
                handlers,
                qps_limit,
                max_body_bytes,
                RPC_API_PATH,
                RPC_API_SERVER_NAME,
                CorsOrigin::Specified(cors_origin),
            )
            .await
        }
    }
}
