//! RPCs related to the state.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{convert::TryFrom, str};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use lazy_static::lazy_static;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use warp_json_rpc::Builder;

use casper_execution_engine::{
    core::engine_state::{BalanceResult, QueryResult},
    storage::protocol_data::ProtocolData,
};
use casper_types::{bytesrepr::ToBytes, CLValue, Key, ProtocolVersion, URef, U512};

use super::{
    docs::DocExample, Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithParams, RpcWithParamsExt,
};
use crate::{
    components::CLIENT_API_VERSION,
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    rpcs::{RpcWithoutParams, RpcWithoutParamsExt},
    types::{
        json_compatibility::{AuctionState, StoredValue},
        Block,
    },
};

lazy_static! {
    static ref MERKLE_PROOF: String = String::from(
        "01000000006ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a72536147614625016ef2e0949ac76e\
        55812421f755abe129b6244fe7168b77f47a72536147614625000000003529cde5c621f857f75f3810611eb4af3\
        f998caaa9d4a3413cf799f99c67db0307010000006ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a\
        7253614761462501010102000000006e06000000000074769d28aac597a36a03a932d4b43e4f10bf0403ee5c41d\
        d035102553f5773631200b9e173e8f05361b681513c14e25e3138639eb03232581db7557c9e8dbbc83ce9450022\
        6a9a7fe4f2b7b88d5103a4fc7400f02bf89c860c9ccdd56951a2afe9be0e0267006d820fb5676eb2960e15722f7\
        725f3f8f41030078f8b2e44bf0dc03f71b176d6e800dc5ae9805068c5be6da1a90b2528ee85db0609cc0fb4bd60\
        bbd559f497a98b67f500e1e3e846592f4918234647fca39830b7e1e6ad6f5b7a99b39af823d82ba1873d0000030\
        00000010186ff500f287e9b53f823ae1582b1fa429dfede28015125fd233a31ca04d5012002015cc42669a55467\
        a1fdf49750772bfc1aed59b9b085558eb81510e9b015a7c83b0301e3cf4a34b1db6bfa58808b686cb8fe21ebe0c\
        1bcbcee522649d2b135fe510fe3");
    static ref GET_ITEM_PARAMS: GetItemParams = GetItemParams {
        state_root_hash: *Block::doc_example().header().state_root_hash(),
        key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1".to_string(),
        path: vec!["inner".to_string()]
    };
    static ref GET_ITEM_RESULT: GetItemResult = GetItemResult {
        api_version: CLIENT_API_VERSION.clone(),
        stored_value: StoredValue::CLValue(CLValue::from_t(1u64).unwrap()),
        merkle_proof: MERKLE_PROOF.clone(),
    };
    static ref GET_BALANCE_PARAMS: GetBalanceParams = GetBalanceParams {
        state_root_hash: *Block::doc_example().header().state_root_hash(),
        purse_uref: "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007".to_string(),
    };
    static ref GET_BALANCE_RESULT: GetBalanceResult = GetBalanceResult {
        api_version: CLIENT_API_VERSION.clone(),
        balance_value: U512::from(123_456),
        merkle_proof: MERKLE_PROOF.clone(),
    };
    static ref GET_AUCTION_INFO_RESULT: GetAuctionInfoResult = GetAuctionInfoResult {
        api_version: CLIENT_API_VERSION.clone(),
        auction_state: AuctionState::doc_example().clone(),
    };
}

/// Params for "state_get_item" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetItemParams {
    /// Hash of the state root.
    pub state_root_hash: Digest,
    /// `casper_types::Key` as formatted string.
    pub key: String,
    /// The path components starting from the key as base.
    #[serde(default)]
    pub path: Vec<String>,
}

impl DocExample for GetItemParams {
    fn doc_example() -> &'static Self {
        &*GET_ITEM_PARAMS
    }
}

/// Result for "state_get_item" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetItemResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: Version,
    /// The stored value.
    pub stored_value: StoredValue,
    /// The merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetItemResult {
    fn doc_example() -> &'static Self {
        &*GET_ITEM_RESULT
    }
}

/// "state_get_item" RPC.
pub struct GetItem {}

impl RpcWithParams for GetItem {
    const METHOD: &'static str = "state_get_item";
    type RequestParams = GetItemParams;
    type ResponseResult = GetItemResult;
}

impl RpcWithParamsExt for GetItem {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Try to parse a `casper_types::Key` from the params.
            let base_key = match Key::from_formatted_str(&params.key)
                .map_err(|error| format!("failed to parse key: {:?}", error))
            {
                Ok(key) => key,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseQueryKey as i64,
                        error_msg,
                    ))?);
                }
            };

            // Run the query.
            let query_result = effect_builder
                .make_request(
                    |responder| RpcRequest::QueryGlobalState {
                        state_root_hash: params.state_root_hash,
                        base_key,
                        path: params.path,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            // Extract the EE `(StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>)` from the
            // result.
            let (value, proof) = match query_result {
                Ok(QueryResult::Success { value, proofs }) => (value, proofs),
                Ok(query_result) => {
                    let error_msg = format!("state query failed: {:?}", query_result);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::QueryFailed as i64,
                        error_msg,
                    ))?);
                }
                Err(error) => {
                    let error_msg = format!("state query failed to execute: {:?}", error);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::QueryFailedToExecute as i64,
                        error_msg,
                    ))?);
                }
            };

            let value_compat = match StoredValue::try_from(&*value) {
                Ok(value_compat) => value_compat,
                Err(error) => {
                    info!("failed to encode stored value: {}", error);
                    return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                }
            };

            let proof_bytes = match proof.to_bytes() {
                Ok(proof_bytes) => proof_bytes,
                Err(error) => {
                    info!("failed to encode stored value: {}", error);
                    return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                }
            };

            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                stored_value: value_compat,
                merkle_proof: hex::encode(proof_bytes),
            };

            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Params for "state_get_balance" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBalanceParams {
    /// The hash of state root.
    pub state_root_hash: Digest,
    /// Formatted URef.
    pub purse_uref: String,
}

impl DocExample for GetBalanceParams {
    fn doc_example() -> &'static Self {
        &*GET_BALANCE_PARAMS
    }
}

/// Result for "state_get_balance" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBalanceResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: Version,
    /// The balance value.
    pub balance_value: U512,
    /// The merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetBalanceResult {
    fn doc_example() -> &'static Self {
        &*GET_BALANCE_RESULT
    }
}

/// "state_get_balance" RPC.
pub struct GetBalance {}

impl RpcWithParams for GetBalance {
    const METHOD: &'static str = "state_get_balance";
    type RequestParams = GetBalanceParams;
    type ResponseResult = GetBalanceResult;
}

impl RpcWithParamsExt for GetBalance {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Try to parse the purse's URef from the params.
            let purse_uref = match URef::from_formatted_str(&params.purse_uref)
                .map_err(|error| format!("failed to parse purse_uref: {:?}", error))
            {
                Ok(uref) => uref,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseGetBalanceURef as i64,
                        error_msg,
                    ))?);
                }
            };

            // Get the balance.
            let balance_result = effect_builder
                .make_request(
                    |responder| RpcRequest::GetBalance {
                        state_root_hash: params.state_root_hash,
                        purse_uref,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let (balance_value, purse_proof, balance_proof) = match balance_result {
                Ok(BalanceResult::Success {
                    motes,
                    purse_proof,
                    balance_proof,
                }) => (motes, purse_proof, balance_proof),
                Ok(balance_result) => {
                    let error_msg = format!("get-balance failed: {:?}", balance_result);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::GetBalanceFailed as i64,
                        error_msg,
                    ))?);
                }
                Err(error) => {
                    let error_msg = format!("get-balance failed to execute: {}", error);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::GetBalanceFailedToExecute as i64,
                        error_msg,
                    ))?);
                }
            };

            let proof_bytes = match (*purse_proof, *balance_proof).to_bytes() {
                Ok(proof_bytes) => proof_bytes,
                Err(error) => {
                    info!("failed to encode stored value: {}", error);
                    return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                }
            };

            let merkle_proof = hex::encode(proof_bytes);

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                balance_value,
                merkle_proof,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Result for "state_get_auction_info" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAuctionInfoResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: Version,
    /// The auction state.
    pub auction_state: AuctionState,
}

impl DocExample for GetAuctionInfoResult {
    fn doc_example() -> &'static Self {
        &*GET_AUCTION_INFO_RESULT
    }
}

/// "state_get_auction_info" RPC.
pub struct GetAuctionInfo {}

impl RpcWithoutParams for GetAuctionInfo {
    const METHOD: &'static str = "state_get_auction_info";
    type ResponseResult = GetAuctionInfoResult;
}

impl RpcWithoutParamsExt for GetAuctionInfo {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let block: Block = {
                let maybe_block = effect_builder
                    .make_request(
                        |responder| RpcRequest::GetBlock {
                            maybe_id: None,
                            responder,
                        },
                        QueueKind::Api,
                    )
                    .await;

                match maybe_block {
                    None => {
                        let error_msg =
                            "get-auction-info failed to get last added block".to_string();
                        info!("{}", error_msg);
                        return Ok(response_builder.error(warp_json_rpc::Error::custom(
                            ErrorCode::NoSuchBlock as i64,
                            error_msg,
                        ))?);
                    }
                    Some(block) => block,
                }
            };

            let protocol_version = ProtocolVersion::V1_0_0;
            let protocol_version_result = effect_builder
                .make_request(
                    |responder| RpcRequest::QueryProtocolData {
                        protocol_version,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let protocol_data = {
                if let Ok(Some(protocol_data)) = protocol_version_result {
                    protocol_data
                } else {
                    Box::new(ProtocolData::default())
                }
            };

            // auction contract key
            let base_key = protocol_data.auction().into();
            // bids named key in auction contract
            let path = vec![casper_types::auction::BIDS_KEY.to_string()];
            // the global state hash of the last block
            let state_root_hash = *block.header().state_root_hash();
            // the block height of the last added block
            let block_height = block.header().height();

            let query_result = effect_builder
                .make_request(
                    |responder| RpcRequest::QueryGlobalState {
                        state_root_hash,
                        base_key,
                        path,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let bids = if let Ok(QueryResult::Success { value, .. }) = query_result {
                value
                    .as_cl_value()
                    .and_then(|cl_value| cl_value.to_owned().into_t().ok())
            } else {
                None
            };

            let era_validators_result = effect_builder
                .make_request(
                    |responder| RpcRequest::QueryEraValidators {
                        state_root_hash,
                        protocol_version,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let era_validators = era_validators_result.ok();

            let auction_state =
                AuctionState::new(state_root_hash, block_height, era_validators, bids);
            debug!("responding to client with: {:?}", auction_state);
            Ok(response_builder.success(auction_state)?)
        }
        .boxed()
    }
}
