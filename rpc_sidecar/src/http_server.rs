use std::sync::Arc;

use hyper::server::{conn::AddrIncoming, Builder};

use casper_json_rpc::{CorsOrigin, RequestHandlersBuilder};
use casper_types::ProtocolVersion;

use crate::node_interface::NodeInterface;

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
    node: Arc<dyn NodeInterface>,
    builder: Builder<AddrIncoming>,
    api_version: ProtocolVersion,
    qps_limit: u64,
    max_body_bytes: u32,
    cors_origin: String,
) {
    let mut handlers = RequestHandlersBuilder::new();
    PutDeploy::register_as_handler(node.clone(), api_version, &mut handlers);
    PutTransaction::register_as_handler(node.clone(), api_version, &mut handlers);
    GetBlock::register_as_handler(node.clone(), api_version, &mut handlers);
    GetBlockTransfers::register_as_handler(node.clone(), api_version, &mut handlers);
    GetStateRootHash::register_as_handler(node.clone(), api_version, &mut handlers);
    GetItem::register_as_handler(node.clone(), api_version, &mut handlers);
    QueryGlobalState::register_as_handler(node.clone(), api_version, &mut handlers);
    GetBalance::register_as_handler(node.clone(), api_version, &mut handlers);
    GetAccountInfo::register_as_handler(node.clone(), api_version, &mut handlers);
    GetDeploy::register_as_handler(node.clone(), api_version, &mut handlers);
    // TODO: handle peers and status
    // GetPeers::register_as_handler(node_interface.clone(), api_version, &mut handlers);
    // GetStatus::register_as_handler(node_interface.clone(), api_version, &mut handlers);
    GetEraInfoBySwitchBlock::register_as_handler(node.clone(), api_version, &mut handlers);
    GetEraSummary::register_as_handler(node.clone(), api_version, &mut handlers);
    GetAuctionInfo::register_as_handler(node.clone(), api_version, &mut handlers);
    GetTrie::register_as_handler(node.clone(), api_version, &mut handlers);
    GetValidatorChanges::register_as_handler(node.clone(), api_version, &mut handlers);
    ListRpcs::register_as_handler(node.clone(), api_version, &mut handlers);
    GetDictionaryItem::register_as_handler(node.clone(), api_version, &mut handlers);
    GetChainspec::register_as_handler(node.clone(), api_version, &mut handlers);
    QueryBalance::register_as_handler(node, api_version, &mut handlers);
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
