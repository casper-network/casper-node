use hyper::server::{conn::AddrIncoming, Builder};

use casper_json_rpc::RequestHandlersBuilder;
use casper_types::ProtocolVersion;

use super::{
    rpcs::{
        account::PutDeploy,
        chain::{GetBlock, GetBlockTransfers, GetEraInfoBySwitchBlock, GetStateRootHash},
        docs::ListRpcs,
        info::{GetChainspec, GetDeploy, GetPeers, GetStatus, GetValidatorChanges},
        state::{
            GetAccountInfo, GetAuctionInfo, GetBalance, GetDictionaryItem, GetItem, GetTrie,
            QueryBalance, QueryGlobalState,
        },
        RpcWithOptionalParams, RpcWithParams, RpcWithoutParams,
    },
    ReactorEventT,
};
use crate::effect::EffectBuilder;

/// The URL path for all JSON-RPC requests.
pub const RPC_API_PATH: &str = "rpc";

pub const RPC_API_SERVER_NAME: &str = "JSON RPC";

/// Run the JSON-RPC server.
pub(super) async fn run<REv: ReactorEventT>(
    builder: Builder<AddrIncoming>,
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
    qps_limit: u64,
    max_body_bytes: u32,
) {
    let mut handlers = RequestHandlersBuilder::new();
    PutDeploy::register_as_handler(effect_builder, api_version, &mut handlers);
    GetBlock::register_as_handler(effect_builder, api_version, &mut handlers);
    GetBlockTransfers::register_as_handler(effect_builder, api_version, &mut handlers);
    GetStateRootHash::register_as_handler(effect_builder, api_version, &mut handlers);
    GetItem::register_as_handler(effect_builder, api_version, &mut handlers);
    QueryGlobalState::register_as_handler(effect_builder, api_version, &mut handlers);
    GetBalance::register_as_handler(effect_builder, api_version, &mut handlers);
    GetAccountInfo::register_as_handler(effect_builder, api_version, &mut handlers);
    GetDeploy::register_as_handler(effect_builder, api_version, &mut handlers);
    GetPeers::register_as_handler(effect_builder, api_version, &mut handlers);
    GetStatus::register_as_handler(effect_builder, api_version, &mut handlers);
    GetEraInfoBySwitchBlock::register_as_handler(effect_builder, api_version, &mut handlers);
    GetAuctionInfo::register_as_handler(effect_builder, api_version, &mut handlers);
    GetTrie::register_as_handler(effect_builder, api_version, &mut handlers);
    GetValidatorChanges::register_as_handler(effect_builder, api_version, &mut handlers);
    ListRpcs::register_as_handler(effect_builder, api_version, &mut handlers);
    GetDictionaryItem::register_as_handler(effect_builder, api_version, &mut handlers);
    GetChainspec::register_as_handler(effect_builder, api_version, &mut handlers);
    QueryBalance::register_as_handler(effect_builder, api_version, &mut handlers);
    let handlers = handlers.build();

    super::rpcs::run(
        builder,
        handlers,
        qps_limit,
        max_body_bytes,
        RPC_API_PATH,
        RPC_API_SERVER_NAME,
    )
    .await;
}
