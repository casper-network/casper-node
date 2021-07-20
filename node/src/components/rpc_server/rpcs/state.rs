//! RPCs related to the state.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use warp_json_rpc::Builder;

use casper_execution_engine::core::engine_state::{BalanceResult, GetBidsResult};
use casper_types::{
    bytesrepr::ToBytes, CLType, CLValue, Key, ProtocolVersion, PublicKey, SecretKey, URef, U512,
};

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithParams, RpcWithParamsExt,
};
use crate::{
    components::rpc_server::rpcs::RpcWithOptionalParams,
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    rpcs::{
        chain::BlockIdentifier,
        common::{self, MERKLE_PROOF},
        RpcWithOptionalParamsExt,
    },
    types::{
        json_compatibility::{Account, AuctionState, StoredValue},
        Block,
    },
};

static GET_ITEM_PARAMS: Lazy<GetItemParams> = Lazy::new(|| GetItemParams {
    state_root_hash: *Block::doc_example().header().state_root_hash(),
    key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1".to_string(),
    path: vec!["inner".to_string()],
});
static GET_ITEM_RESULT: Lazy<GetItemResult> = Lazy::new(|| GetItemResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    stored_value: StoredValue::CLValue(CLValue::from_t(1u64).unwrap()),
    merkle_proof: MERKLE_PROOF.clone(),
});
static GET_BALANCE_PARAMS: Lazy<GetBalanceParams> = Lazy::new(|| GetBalanceParams {
    state_root_hash: *Block::doc_example().header().state_root_hash(),
    purse_uref: "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007"
        .to_string(),
});
static GET_BALANCE_RESULT: Lazy<GetBalanceResult> = Lazy::new(|| GetBalanceResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    balance_value: U512::from(123_456),
    merkle_proof: MERKLE_PROOF.clone(),
});
static GET_AUCTION_INFO_PARAMS: Lazy<GetAuctionInfoParams> = Lazy::new(|| GetAuctionInfoParams {
    block_identifier: BlockIdentifier::Hash(*Block::doc_example().hash()),
});
static GET_AUCTION_INFO_RESULT: Lazy<GetAuctionInfoResult> = Lazy::new(|| GetAuctionInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    auction_state: AuctionState::doc_example().clone(),
});
static GET_ACCOUNT_INFO_PARAMS: Lazy<GetAccountInfoParams> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key = PublicKey::from(&secret_key);
    GetAccountInfoParams {
        public_key,
        block_identifier: Some(BlockIdentifier::Hash(*Block::doc_example().hash())),
    }
});
static GET_ACCOUNT_INFO_RESULT: Lazy<GetAccountInfoResult> = Lazy::new(|| GetAccountInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    account: Account::doc_example().clone(),
    merkle_proof: MERKLE_PROOF.clone(),
});
static GET_DICTIONARY_ITEM_PARAMS: Lazy<GetDictionaryItemParams> =
    Lazy::new(|| GetDictionaryItemParams {
        state_root_hash: *Block::doc_example().header().state_root_hash(),
        dictionary_identifier: DictionaryIdentifier::URef {
            seed_uref: "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007"
                .to_string(),
            dictionary_item_key: "a_unique_entry_identifier".to_string(),
        },
    });
static GET_DICTIONARY_ITEM_RESULT: Lazy<GetDictionaryItemResult> =
    Lazy::new(|| GetDictionaryItemResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        dictionary_key:
            "dictionary-67518854aa916c97d4e53df8570c8217ccc259da2721b692102d76acd0ee8d1f"
                .to_string(),
        stored_value: StoredValue::CLValue(CLValue::from_t(1u64).unwrap()),
        merkle_proof: MERKLE_PROOF.clone(),
    });

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
    pub api_version: ProtocolVersion,
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
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Try to parse a `casper_types::Key` from the params.
            let base_key = match Key::from_formatted_str(&params.key)
                .map_err(|error| format!("failed to parse key: {}", error))
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

            let (stored_value, proof_bytes) = match common::extract_query_result(query_result) {
                Ok(tuple) => tuple,
                Err((error_code, error_msg)) => {
                    info!("{}", error_msg);
                    return Ok(response_builder
                        .error(warp_json_rpc::Error::custom(error_code as i64, error_msg))?);
                }
            };

            let result = Self::ResponseResult {
                api_version,
                stored_value,
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
    pub api_version: ProtocolVersion,
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
        api_version: ProtocolVersion,
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

            let (balance_value, balance_proof) = match balance_result {
                Ok(BalanceResult::Success { motes, proof }) => (motes, proof),
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

            let proof_bytes = match balance_proof.to_bytes() {
                Ok(proof_bytes) => proof_bytes,
                Err(error) => {
                    info!("failed to encode stored value: {}", error);
                    return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                }
            };

            let merkle_proof = hex::encode(proof_bytes);

            // Return the result.
            let result = Self::ResponseResult {
                api_version,
                balance_value,
                merkle_proof,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Params for "state_get_auction_info" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAuctionInfoParams {
    /// The block identifier.
    pub block_identifier: BlockIdentifier,
}

impl DocExample for GetAuctionInfoParams {
    fn doc_example() -> &'static Self {
        &*GET_AUCTION_INFO_PARAMS
    }
}

/// Result for "state_get_auction_info" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAuctionInfoResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
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

impl RpcWithOptionalParams for GetAuctionInfo {
    const METHOD: &'static str = "state_get_auction_info";
    type OptionalRequestParams = GetAuctionInfoParams;
    type ResponseResult = GetAuctionInfoResult;
}

impl RpcWithOptionalParamsExt for GetAuctionInfo {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let maybe_id = maybe_params.map(|params| params.block_identifier);
            let block: Block = {
                let maybe_block = effect_builder
                    .make_request(
                        |responder| RpcRequest::GetBlock {
                            maybe_id,
                            responder,
                        },
                        QueueKind::Api,
                    )
                    .await;

                match maybe_block {
                    None => {
                        let error_msg = if maybe_id.is_none() {
                            "get-auction-info failed to get last added block".to_string()
                        } else {
                            "get-auction-info failed to get specified block".to_string()
                        };
                        info!("{}", error_msg);
                        return Ok(response_builder.error(warp_json_rpc::Error::custom(
                            ErrorCode::NoSuchBlock as i64,
                            error_msg,
                        ))?);
                    }
                    Some((block, _)) => block,
                }
            };

            let protocol_version = api_version;

            // the global state hash of the last block
            let state_root_hash = *block.header().state_root_hash();
            // the block height of the last added block
            let block_height = block.header().height();

            let get_bids_result = effect_builder
                .make_request(
                    |responder| RpcRequest::GetBids {
                        state_root_hash,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let maybe_bids = if let Ok(GetBidsResult::Success { bids, .. }) = get_bids_result {
                Some(bids)
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
                AuctionState::new(state_root_hash, block_height, era_validators, maybe_bids);

            let result = Self::ResponseResult {
                api_version,
                auction_state,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Params for "state_get_account_info" RPC request
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAccountInfoParams {
    /// The public key of the Account.
    pub public_key: PublicKey,
    /// The block identifier.
    pub block_identifier: Option<BlockIdentifier>,
}

impl DocExample for GetAccountInfoParams {
    fn doc_example() -> &'static Self {
        &*GET_ACCOUNT_INFO_PARAMS
    }
}

/// Result for "state_get_account_info" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAccountInfoResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The account.
    pub account: Account,
    /// The merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetAccountInfoResult {
    fn doc_example() -> &'static Self {
        &*GET_ACCOUNT_INFO_RESULT
    }
}

/// "state_get_account_info" RPC.
pub struct GetAccountInfo {}

impl RpcWithParams for GetAccountInfo {
    const METHOD: &'static str = "state_get_account_info";
    type RequestParams = GetAccountInfoParams;
    type ResponseResult = GetAccountInfoResult;
}

impl RpcWithParamsExt for GetAccountInfo {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let base_key = {
                let account_hash = params.public_key.to_account_hash();
                Key::Account(account_hash)
            };

            let block: Block = {
                let maybe_id = params.block_identifier;
                let maybe_block = effect_builder
                    .make_request(
                        |responder| RpcRequest::GetBlock {
                            maybe_id,
                            responder,
                        },
                        QueueKind::Api,
                    )
                    .await;

                match maybe_block {
                    None => {
                        let error_msg = if maybe_id.is_none() {
                            "get-account-info failed to get last added block".to_string()
                        } else {
                            "get-account-info failed to get specified block".to_string()
                        };
                        info!("{}", error_msg);
                        return Ok(response_builder.error(warp_json_rpc::Error::custom(
                            ErrorCode::NoSuchBlock as i64,
                            error_msg,
                        ))?);
                    }
                    Some((block, _)) => block,
                }
            };

            let state_root_hash = *block.header().state_root_hash();

            let query_result = effect_builder
                .make_request(
                    |responder| RpcRequest::QueryGlobalState {
                        state_root_hash,
                        base_key,
                        path: vec![],
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let (stored_value, proof_bytes) = match common::extract_query_result(query_result) {
                Ok(tuple) => tuple,
                Err((error_code, error_msg)) => {
                    info!("{}", error_msg);
                    return Ok(response_builder
                        .error(warp_json_rpc::Error::custom(error_code as i64, error_msg))?);
                }
            };

            let account = if let StoredValue::Account(account) = stored_value {
                account
            } else {
                let error_msg = "get-account-info failed to get specified account".to_string();
                return Ok(response_builder.error(warp_json_rpc::Error::custom(
                    ErrorCode::NoSuchAccount as i64,
                    error_msg,
                ))?);
            };

            let result = Self::ResponseResult {
                api_version,
                account,
                merkle_proof: hex::encode(proof_bytes),
            };

            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
/// Options for dictionary item lookups.
pub enum DictionaryIdentifier {
    /// Lookup a dictionary item via an Account's named keys.
    AccountNamedKey {
        /// The account key as a formatted string whose named keys contains dictionary_name.
        key: String,
        /// The named key under which the dictionary seed URef is stored.
        dictionary_name: String,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via a Contract's named keys.
    ContractNamedKey {
        /// The contract key as a formatted string whose named keys contains dictionary_name.
        key: String,
        /// The named key under which the dictionary seed URef is stored.
        dictionary_name: String,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via its seed URef.
    URef {
        /// The dictionary's seed URef.
        seed_uref: String,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via its unique key.
    Dictionary(String),
}

impl DictionaryIdentifier {
    fn get_dictionary_base_key(&self) -> Result<Option<Key>, Error> {
        match self {
            DictionaryIdentifier::AccountNamedKey { ref key, .. }
            | DictionaryIdentifier::ContractNamedKey { ref key, .. } => {
                match Key::from_formatted_str(key) {
                    Ok(key) => Ok(Some(key)),
                    Err(error) => Err(Error(format!("failed to parse key: {}", error))),
                }
            }
            DictionaryIdentifier::URef { .. } | DictionaryIdentifier::Dictionary(_) => Ok(None),
        }
    }

    fn get_base_query_path(&self) -> Result<Option<Vec<String>>, Error> {
        match self {
            DictionaryIdentifier::AccountNamedKey {
                dictionary_name, ..
            }
            | DictionaryIdentifier::ContractNamedKey {
                dictionary_name, ..
            } => Ok(Some(vec![dictionary_name.clone()])),
            DictionaryIdentifier::URef { .. } | DictionaryIdentifier::Dictionary(_) => Ok(None),
        }
    }

    fn get_dictionary_address(
        &self,
        maybe_stored_value: Option<StoredValue>,
    ) -> Result<Key, Error> {
        match self {
            DictionaryIdentifier::AccountNamedKey {
                dictionary_item_key,
                ..
            }
            | DictionaryIdentifier::ContractNamedKey {
                dictionary_item_key,
                ..
            } => match maybe_stored_value {
                Some(StoredValue::CLValue(value)) => {
                    if *value.cl_type() == CLType::URef {
                        let seed: URef = value
                            .into_t()
                            .map_err(|_| Error("Failed to parse URef".to_string()))?;
                        let key_bytes = dictionary_item_key.as_str().as_bytes();
                        Ok(Key::dictionary(seed, key_bytes))
                    } else {
                        Err(Error("Failed create dictionary address".to_string()))
                    }
                }
                Some(_) | None => Err(Error("Failed to create dictionary address".to_string())),
            },
            DictionaryIdentifier::URef {
                seed_uref,
                dictionary_item_key,
            } => {
                let key_bytes = dictionary_item_key.as_str().as_bytes();
                let seed_uref = URef::from_formatted_str(seed_uref)
                    .map_err(|_| Error("Failed to parse URef".to_string()))?;
                Ok(Key::dictionary(seed_uref, key_bytes))
            }
            DictionaryIdentifier::Dictionary(address) => Key::from_formatted_str(address)
                .map_err(|_| Error("Failed to parse Dictionary key".to_string())),
        }
    }
}

/// Params for "state_get_dict" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDictionaryItemParams {
    /// Hash of the state root
    pub state_root_hash: Digest,
    /// The Dictionary query identifier.
    pub dictionary_identifier: DictionaryIdentifier,
}

impl DocExample for GetDictionaryItemParams {
    fn doc_example() -> &'static Self {
        &*GET_DICTIONARY_ITEM_PARAMS
    }
}

/// Result for "state_get_dictionary" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDictionaryItemResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The key under which the value is stored.
    pub dictionary_key: String,
    /// The stored value.
    pub stored_value: StoredValue,
    /// The merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetDictionaryItemResult {
    fn doc_example() -> &'static Self {
        &*GET_DICTIONARY_ITEM_RESULT
    }
}

/// "state_get_dictionary" RPC.
pub struct GetDictionaryItem {}

impl RpcWithParams for GetDictionaryItem {
    const METHOD: &'static str = "state_get_dictionary_item";
    type RequestParams = GetDictionaryItemParams;
    type ResponseResult = GetDictionaryItemResult;
}

impl RpcWithParamsExt for GetDictionaryItem {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let dictionary_address = match params.dictionary_identifier {
                DictionaryIdentifier::AccountNamedKey { .. }
                | DictionaryIdentifier::ContractNamedKey { .. } => {
                    let base_key = match params.dictionary_identifier.get_dictionary_base_key() {
                        Ok(Some(key)) => key,
                        Err(_) | Ok(None) => {
                            error!("Failed to parse key");
                            return Ok(response_builder.error(warp_json_rpc::Error::custom(
                                ErrorCode::ParseQueryKey as i64,
                                "Failed to parse key",
                            ))?);
                        }
                    };

                    let path = match params.dictionary_identifier.get_base_query_path() {
                        Ok(Some(path)) => path,
                        Err(_) | Ok(None) => {
                            error!("Failed to execute query");
                            return Ok(response_builder.error(warp_json_rpc::Error::custom(
                                ErrorCode::NoDictionaryName as i64,
                                "Failed to execute query",
                            ))?);
                        }
                    };

                    let query_result = effect_builder
                        .make_request(
                            |responder| RpcRequest::QueryGlobalState {
                                state_root_hash: params.state_root_hash,
                                base_key,
                                path,
                                responder,
                            },
                            QueueKind::Api,
                        )
                        .await;

                    let (stored_value, _) = match common::extract_query_result(query_result) {
                        Ok(tuple) => tuple,
                        Err((error_code, error_msg)) => {
                            info!("{}", error_msg);
                            return Ok(response_builder.error(warp_json_rpc::Error::custom(
                                error_code as i64,
                                error_msg,
                            ))?);
                        }
                    };

                    params
                        .dictionary_identifier
                        .get_dictionary_address(Some(stored_value))
                }
                DictionaryIdentifier::URef { .. } | DictionaryIdentifier::Dictionary(_) => {
                    params.dictionary_identifier.get_dictionary_address(None)
                }
            };

            let dictionary_query_key = match dictionary_address {
                Ok(key) => key,
                Err(Error(message)) => {
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::FailedToGetDictionaryURef as i64,
                        message,
                    ))?)
                }
            };

            let query_result = effect_builder
                .make_request(
                    |responder| RpcRequest::QueryGlobalState {
                        state_root_hash: params.state_root_hash,
                        base_key: dictionary_query_key,
                        path: vec![],
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let (stored_value, proof_bytes) = match common::extract_query_result(query_result) {
                Ok(tuple) => tuple,
                Err((error_code, error_msg)) => {
                    info!("{}", error_msg);
                    return Ok(response_builder
                        .error(warp_json_rpc::Error::custom(error_code as i64, error_msg))?);
                }
            };

            let result = Self::ResponseResult {
                api_version,
                dictionary_key: dictionary_query_key.to_formatted_string(),
                stored_value,
                merkle_proof: hex::encode(proof_bytes),
            };

            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
