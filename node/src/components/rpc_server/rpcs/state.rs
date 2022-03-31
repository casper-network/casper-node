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

use casper_execution_engine::{
    core::engine_state::{BalanceResult, GetBidsResult, QueryResult},
    storage::trie::merkle_proof::TrieMerkleProof,
};
use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    CLValue, Key, ProtocolVersion, PublicKey, SecretKey, StoredValue as DomainStoredValue, URef,
    U512,
};

use crate::{
    components::rpc_server::rpcs::RpcWithOptionalParams,
    effect::EffectBuilder,
    reactor::QueueKind,
    rpcs::{
        chain::BlockIdentifier,
        common::{self, MERKLE_PROOF},
        docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
        Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithOptionalParamsExt, RpcWithParams,
        RpcWithParamsExt,
    },
    types::{
        json_compatibility::{Account as JsonAccount, AuctionState, StoredValue},
        Block, BlockHash, JsonBlockHeader,
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
    account: JsonAccount::doc_example().clone(),
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
static QUERY_GLOBAL_STATE_PARAMS: Lazy<QueryGlobalStateParams> =
    Lazy::new(|| QueryGlobalStateParams {
        state_identifier: GlobalStateIdentifier::BlockHash(*Block::doc_example().hash()),
        key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1".to_string(),
        path: vec![],
    });
static QUERY_GLOBAL_STATE_RESULT: Lazy<QueryGlobalStateResult> =
    Lazy::new(|| QueryGlobalStateResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        block_header: Some(JsonBlockHeader::doc_example().clone()),
        stored_value: StoredValue::Account(JsonAccount::doc_example().clone()),
        merkle_proof: MERKLE_PROOF.clone(),
    });
static GET_TRIE_PARAMS: Lazy<GetTrieParams> = Lazy::new(|| GetTrieParams {
    trie_key: *Block::doc_example().header().state_root_hash(),
});
static GET_TRIE_RESULT: Lazy<GetTrieResult> = Lazy::new(|| GetTrieResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    maybe_trie_bytes: None,
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
            let query_result = common::run_query_and_encode(
                effect_builder,
                params.state_root_hash,
                base_key,
                params.path,
            )
            .await;

            match query_result {
                Ok((stored_value, merkle_proof)) => {
                    let result = Self::ResponseResult {
                        api_version,
                        stored_value,
                        merkle_proof,
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => Ok(response_builder.error(error)?),
            }
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

            let merkle_proof = base16::encode_lower(&proof_bytes);

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
            // This RPC request is restricted by the block availability index.
            let only_from_available_block_range = true;

            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
            let block = match common::get_block(maybe_block_id, only_from_available_block_range, effect_builder).await {
                Ok(block) => block,
                Err(error) => return Ok(response_builder.error(error)?),
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

            let bids = match get_bids_result {
                Ok(GetBidsResult::Success { bids }) => bids,
                Ok(GetBidsResult::RootNotFound) => {
                    error!(
                        block_hash=?block.hash(),
                        ?state_root_hash,
                        "root not found while trying to get bids"
                    );
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::InternalError as i64,
                        format!("failed to get bids at block {:?}", block.hash().inner()),
                    ))?);
                }
                Err(error) => {
                    error!(
                        block_hash=?block.hash(),
                        ?state_root_hash,
                        ?error,
                        "failed to get bids"
                    );
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::InternalError as i64,
                        format!("error getting bids at block {:?}", block.hash().inner()),
                    ))?);
                }
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

            let era_validators = match era_validators_result {
                Ok(validators) => validators,
                Err(err) => {
                    error!(block_hash=?block.hash(), ?state_root_hash, ?err, "failed to get era validators");
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::InternalError as i64,
                        format!("failed to get validators at block {:?}", block.hash().inner()),
                    ))?);
                }
            };

            let auction_state =
                AuctionState::new(state_root_hash, block_height, era_validators, bids);

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
    pub account: JsonAccount,
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
            // This RPC request is restricted by the block availability index.
            let only_from_available_block_range = true;

            let maybe_block_id = params.block_identifier;
            let block = match common::get_block(
                maybe_block_id,
                only_from_available_block_range,
                effect_builder,
            )
            .await
            {
                Ok(block) => block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            let state_root_hash = *block.header().state_root_hash();
            let base_key = {
                let account_hash = params.public_key.to_account_hash();
                Key::Account(account_hash)
            };
            let query_result =
                common::run_query_and_encode(effect_builder, state_root_hash, base_key, vec![])
                    .await;

            let (stored_value, merkle_proof) = match query_result {
                Ok(tuple) => tuple,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            let account = if let StoredValue::Account(account) = stored_value {
                account
            } else {
                let error_msg = "failed to get specified account".to_string();
                info!(?stored_value, "{}", error_msg);
                return Ok(response_builder.error(warp_json_rpc::Error::custom(
                    ErrorCode::NoSuchAccount as i64,
                    error_msg,
                ))?);
            };

            let result = Self::ResponseResult {
                api_version,
                account,
                merkle_proof,
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

    fn get_dictionary_address(
        &self,
        maybe_stored_value: Option<DomainStoredValue>,
    ) -> Result<Key, Error> {
        match self {
            DictionaryIdentifier::AccountNamedKey {
                dictionary_name,
                dictionary_item_key,
                ..
            }
            | DictionaryIdentifier::ContractNamedKey {
                dictionary_name,
                dictionary_item_key,
                ..
            } => {
                let named_keys = match &maybe_stored_value {
                    Some(DomainStoredValue::Account(account)) => account.named_keys(),
                    Some(DomainStoredValue::Contract(contract)) => contract.named_keys(),
                    Some(other) => {
                        return Err(Error(format!(
                            "Unexpected StoredValue {}",
                            other.type_name()
                        )))
                    }
                    None => return Err(Error("Could not retrieve account".to_string())),
                };

                let key_bytes = dictionary_item_key.as_str().as_bytes();
                let seed_uref = match named_keys.get(dictionary_name) {
                    Some(key) => *key
                        .as_uref()
                        .ok_or_else(|| Error("Failed to parse key into URef:".to_string()))?,
                    None => return Err(Error("Failed to get seed Uref".to_string())),
                };

                Ok(Key::dictionary(seed_uref, key_bytes))
            }
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

/// Params for "state_get_dictionary_item" RPC request.
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

/// Result for "state_get_dictionary_item" RPC response.
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

/// "state_get_dictionary_item" RPC.
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

                    let empty_path = Vec::new();
                    let value = match run_query(
                        effect_builder,
                        params.state_root_hash,
                        base_key,
                        empty_path,
                    )
                    .await
                    {
                        Ok((value, _proofs)) => value,
                        Err(error) => return Ok(response_builder.error(error)?),
                    };
                    params
                        .dictionary_identifier
                        .get_dictionary_address(Some(value))
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

            let query_result = common::run_query_and_encode(
                effect_builder,
                params.state_root_hash,
                dictionary_query_key,
                vec![],
            )
            .await;

            match query_result {
                Ok((stored_value, merkle_proof)) => {
                    let result = Self::ResponseResult {
                        api_version,
                        dictionary_key: dictionary_query_key.to_formatted_string(),
                        stored_value,
                        merkle_proof,
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => Ok(response_builder.error(error)?),
            }
        }
        .boxed()
    }
}

/// Identifier for possible ways to query Global State
#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub enum GlobalStateIdentifier {
    /// Query using a block hash.
    BlockHash(BlockHash),
    /// Query using the state root hash.
    StateRootHash(Digest),
}

/// Params for "query_global_state" RPC
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryGlobalStateParams {
    /// The identifier used for the query.
    pub state_identifier: GlobalStateIdentifier,
    /// `casper_types::Key` as formatted string.
    pub key: String,
    /// The path components starting from the key as base.
    #[serde(default)]
    pub path: Vec<String>,
}

impl DocExample for QueryGlobalStateParams {
    fn doc_example() -> &'static Self {
        &*QUERY_GLOBAL_STATE_PARAMS
    }
}

/// Result for "query_global_state" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryGlobalStateResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The block header if a Block hash was provided.
    pub block_header: Option<JsonBlockHeader>,
    /// The stored value.
    pub stored_value: StoredValue,
    /// The merkle proof.
    pub merkle_proof: String,
}

impl DocExample for QueryGlobalStateResult {
    fn doc_example() -> &'static Self {
        &*QUERY_GLOBAL_STATE_RESULT
    }
}

/// "query_global_state" RPC
pub struct QueryGlobalState {}

impl RpcWithParams for QueryGlobalState {
    const METHOD: &'static str = "query_global_state";
    type RequestParams = QueryGlobalStateParams;
    type ResponseResult = QueryGlobalStateResult;
}

impl RpcWithParamsExt for QueryGlobalState {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let (state_root_hash, maybe_block_header) = match params.state_identifier {
                GlobalStateIdentifier::BlockHash(block_hash) => {
                    // This RPC request is restricted by the block availability index.
                    let only_from_available_block_range = true;

                    match effect_builder
                        .get_block_header_from_storage(block_hash, only_from_available_block_range)
                        .await
                    {
                        Some(header) => {
                            let json_block_header = JsonBlockHeader::from(header.clone());
                            (*header.state_root_hash(), Some(json_block_header))
                        }
                        None => {
                            let error_msg =
                                "query_global_state failed to retrieve specified block header"
                                    .to_string();
                            return Ok(response_builder.error(warp_json_rpc::Error::custom(
                                ErrorCode::NoSuchBlock as i64,
                                error_msg,
                            ))?);
                        }
                    }
                }
                GlobalStateIdentifier::StateRootHash(state_root_hash) => (state_root_hash, None),
            };

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

            let query_result = common::run_query_and_encode(
                effect_builder,
                state_root_hash,
                base_key,
                params.path,
            )
            .await;

            match query_result {
                Ok((stored_value, merkle_proof)) => {
                    let result = Self::ResponseResult {
                        api_version,
                        block_header: maybe_block_header,
                        stored_value,
                        merkle_proof,
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => Ok(response_builder.error(error)?),
            }
        }
        .boxed()
    }
}

/// Parameters for "state_get_trie" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct GetTrieParams {
    /// A trie key.
    pub trie_key: Digest,
}

impl DocExample for GetTrieParams {
    fn doc_example() -> &'static Self {
        &*GET_TRIE_PARAMS
    }
}

/// Result for "state_get_trie" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTrieResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// A list of keys read under the specified prefix.
    #[schemars(
        with = "Option<String>",
        description = "A trie from global state storage, bytesrepr serialized and hex-encoded."
    )]
    pub maybe_trie_bytes: Option<Bytes>,
}

impl DocExample for GetTrieResult {
    fn doc_example() -> &'static Self {
        &*GET_TRIE_RESULT
    }
}

/// `state_get_trie` RPC.
pub struct GetTrie {}

impl RpcWithParams for GetTrie {
    const METHOD: &'static str = "state_get_trie";
    type RequestParams = GetTrieParams;
    type ResponseResult = GetTrieResult;
}

impl RpcWithParamsExt for GetTrie {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let trie_key = params.trie_key;

            let response = match effect_builder.get_trie_full(trie_key).await {
                Ok(maybe_trie_bytes) => {
                    let result = Self::ResponseResult {
                        api_version,
                        maybe_trie_bytes,
                    };

                    response_builder.success(result)?
                }
                Err(error) => {
                    error!(?error, "failed to get trie");
                    response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::FailedToGetTrie as i64,
                        format!("failed to get trie: {:?}", error),
                    ))?
                }
            };

            Ok(response)
        }
        .boxed()
    }
}

type QuerySuccess = (
    DomainStoredValue,
    Vec<TrieMerkleProof<Key, DomainStoredValue>>,
);

/// Runs a global state query and returns a tuple of the domain stored value and merkle proof of the
/// value.
///
/// On error, a `warp_json_rpc::Error` is returned suitable for sending as a JSON-RPC response.
pub(super) async fn run_query<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    base_key: Key,
    path: Vec<String>,
) -> Result<QuerySuccess, warp_json_rpc::Error> {
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

    match query_result {
        Ok(QueryResult::Success { value, proofs }) => Ok((*value, proofs)),
        Ok(QueryResult::RootNotFound) => {
            info!("query failed: root not found");
            let error = common::missing_block_or_state_root_error(
                effect_builder,
                ErrorCode::NoSuchStateRoot,
                format!("failed to get state root at {:?}", state_root_hash),
            )
            .await;
            Err(error)
        }
        Ok(query_result) => {
            info!(?query_result, "query failed");
            Err(warp_json_rpc::Error::custom(
                ErrorCode::QueryFailed as i64,
                format!("state query failed: {:?}", query_result),
            ))
        }
        Err(error) => {
            info!(?error, "query failed to execute");
            Err(warp_json_rpc::Error::custom(
                ErrorCode::QueryFailedToExecute as i64,
                format!("state query failed to execute: {:?}", error),
            ))
        }
    }
}
