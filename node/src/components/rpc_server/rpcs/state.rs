//! RPCs related to the state.

use std::str;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use casper_json_rpc::ReservedErrorCode;
use casper_storage::{
    data_access_layer::{get_bids::GetBidsResult, BalanceResult, QueryResult},
    global_state::trie::merkle_proof::TrieMerkleProof,
};
use casper_types::{
    account::{Account, AccountHash},
    bytesrepr::{Bytes, ToBytes},
    BlockHash, BlockHeader, BlockV2, CLValue, Digest, Key, ProtocolVersion, PublicKey, SecretKey,
    StoredValue, URef, U512,
};

use crate::{
    effect::EffectBuilder,
    reactor::QueueKind,
    rpcs::{
        chain::BlockIdentifier,
        common::{self, MERKLE_PROOF},
        docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
        Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithOptionalParams, RpcWithParams,
    },
    types::json_compatibility::AuctionState,
};

static GET_ITEM_PARAMS: Lazy<GetItemParams> = Lazy::new(|| GetItemParams {
    state_root_hash: *BlockHeader::example().state_root_hash(),
    key: Key::from_formatted_str(
        "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1",
    )
    .unwrap(),
    path: vec!["inner".to_string()],
});
static GET_ITEM_RESULT: Lazy<GetItemResult> = Lazy::new(|| GetItemResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    stored_value: StoredValue::CLValue(CLValue::from_t(1u64).unwrap()),
    merkle_proof: MERKLE_PROOF.clone(),
});
static GET_BALANCE_PARAMS: Lazy<GetBalanceParams> = Lazy::new(|| GetBalanceParams {
    state_root_hash: *BlockHeader::example().state_root_hash(),
    purse_uref: "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007"
        .to_string(),
});
static GET_BALANCE_RESULT: Lazy<GetBalanceResult> = Lazy::new(|| GetBalanceResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    balance_value: U512::from(123_456),
    merkle_proof: MERKLE_PROOF.clone(),
});
static GET_AUCTION_INFO_PARAMS: Lazy<GetAuctionInfoParams> = Lazy::new(|| GetAuctionInfoParams {
    block_identifier: BlockIdentifier::Hash(*BlockHash::example()),
});
static GET_AUCTION_INFO_RESULT: Lazy<GetAuctionInfoResult> = Lazy::new(|| GetAuctionInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    auction_state: AuctionState::doc_example().clone(),
});
static GET_ACCOUNT_INFO_PARAMS: Lazy<GetAccountInfoParams> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key = PublicKey::from(&secret_key);
    GetAccountInfoParams {
        account_identifier: AccountIdentifier::PublicKey(public_key),
        block_identifier: Some(BlockIdentifier::Hash(*BlockHash::example())),
    }
});
static GET_ACCOUNT_INFO_RESULT: Lazy<GetAccountInfoResult> = Lazy::new(|| GetAccountInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    account: Account::doc_example().clone(),
    merkle_proof: MERKLE_PROOF.clone(),
});
static GET_DICTIONARY_ITEM_PARAMS: Lazy<GetDictionaryItemParams> =
    Lazy::new(|| GetDictionaryItemParams {
        state_root_hash: *BlockHeader::example().state_root_hash(),
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
        state_identifier: Some(GlobalStateIdentifier::BlockHash(*BlockV2::example().hash())),
        key: Key::from_formatted_str(
            "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1",
        )
        .unwrap(),
        path: vec![],
    });
static QUERY_GLOBAL_STATE_RESULT: Lazy<QueryGlobalStateResult> =
    Lazy::new(|| QueryGlobalStateResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        block_header: Some(BlockHeader::example().clone()),
        stored_value: StoredValue::Account(Account::doc_example().clone()),
        merkle_proof: MERKLE_PROOF.clone(),
    });
static GET_TRIE_PARAMS: Lazy<GetTrieParams> = Lazy::new(|| GetTrieParams {
    trie_key: *BlockHeader::example().state_root_hash(),
});
static GET_TRIE_RESULT: Lazy<GetTrieResult> = Lazy::new(|| GetTrieResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    maybe_trie_bytes: None,
});
static QUERY_BALANCE_PARAMS: Lazy<QueryBalanceParams> = Lazy::new(|| QueryBalanceParams {
    state_identifier: Some(GlobalStateIdentifier::BlockHash(*BlockHash::example())),
    purse_identifier: PurseIdentifier::MainPurseUnderAccountHash(AccountHash::new([9u8; 32])),
});
static QUERY_BALANCE_RESULT: Lazy<QueryBalanceResult> = Lazy::new(|| QueryBalanceResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    balance: U512::from(123_456),
});

/// Params for "state_get_item" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetItemParams {
    /// Hash of the state root.
    pub state_root_hash: Digest,
    /// The key under which to query.
    pub key: Key,
    /// The path components starting from the key as base.
    #[serde(default)]
    pub path: Vec<String>,
}

impl DocExample for GetItemParams {
    fn doc_example() -> &'static Self {
        &GET_ITEM_PARAMS
    }
}

/// Result for "state_get_item" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetItemResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The stored value.
    pub stored_value: StoredValue,
    /// The Merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetItemResult {
    fn doc_example() -> &'static Self {
        &GET_ITEM_RESULT
    }
}

/// "state_get_item" RPC.
pub struct GetItem {}

#[async_trait]
impl RpcWithParams for GetItem {
    const METHOD: &'static str = "state_get_item";
    type RequestParams = GetItemParams;
    type ResponseResult = GetItemResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let (stored_value, merkle_proof) = common::run_query_and_encode(
            effect_builder,
            params.state_root_hash,
            params.key,
            params.path,
        )
        .await?;

        let result = Self::ResponseResult {
            api_version,
            stored_value,
            merkle_proof,
        };
        Ok(result)
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
        &GET_BALANCE_PARAMS
    }
}

/// Result for "state_get_balance" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBalanceResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The balance value.
    pub balance_value: U512,
    /// The Merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetBalanceResult {
    fn doc_example() -> &'static Self {
        &GET_BALANCE_RESULT
    }
}

/// "state_get_balance" RPC.
pub struct GetBalance {}

#[async_trait]
impl RpcWithParams for GetBalance {
    const METHOD: &'static str = "state_get_balance";
    type RequestParams = GetBalanceParams;
    type ResponseResult = GetBalanceResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        // Try to parse the purse's URef from the params.
        let purse_uref = match URef::from_formatted_str(&params.purse_uref)
            .map_err(|error| format!("failed to parse purse_uref: {}", error))
        {
            Ok(uref) => uref,
            Err(error_msg) => {
                info!("{}", error_msg);
                return Err(Error::new(
                    ErrorCode::FailedToParseGetBalanceURef,
                    error_msg,
                ));
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

        if let BalanceResult::Success { motes, proof } = balance_result {
            let balance_value = motes;
            let proof_bytes = match proof.to_bytes() {
                Ok(proof_bytes) => proof_bytes,
                Err(error) => {
                    let message = format!("failed to encode proof: {}", error);
                    info!("{}", message);
                    return Err(Error::new(ReservedErrorCode::InternalError, message));
                }
            };

            let merkle_proof = base16::encode_lower(&proof_bytes);

            // Return the result.
            let result = Self::ResponseResult {
                api_version,
                balance_value,
                merkle_proof,
            };
            Ok(result)
        } else {
            info!("get-balance failed: {:?}", balance_result);
            Err(Error::new(
                ErrorCode::FailedToGetBalance,
                format!("failed for purse {}: {:?}", purse_uref, balance_result),
            ))
        }
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
        &GET_AUCTION_INFO_PARAMS
    }
}

/// Result for "state_get_auction_info" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
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
        &GET_AUCTION_INFO_RESULT
    }
}

/// "state_get_auction_info" RPC.
pub struct GetAuctionInfo {}

#[async_trait]
impl RpcWithOptionalParams for GetAuctionInfo {
    const METHOD: &'static str = "state_get_auction_info";
    type OptionalRequestParams = GetAuctionInfoParams;
    type ResponseResult = GetAuctionInfoResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, Error> {
        // This RPC request is restricted by the block availability index.
        let only_from_available_block_range = true;

        let maybe_block_id = maybe_params.map(|params| params.block_identifier);
        let block = common::get_block(
            maybe_block_id,
            only_from_available_block_range,
            effect_builder,
        )
        .await?;

        let protocol_version = api_version;

        // the global state hash of the last block
        let state_root_hash = *block.state_root_hash();
        // the block height of the last added block
        let block_height = block.height();

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
            GetBidsResult::Success { bids } => bids,
            GetBidsResult::RootNotFound => {
                error!(
                    block_hash=?block.hash(),
                    ?state_root_hash,
                    "root not found while trying to get bids"
                );
                return Err(Error::new(
                    ReservedErrorCode::InternalError,
                    format!(
                        "root not found when getting bids at block {:?}",
                        block.hash().inner()
                    ),
                ));
            }
            GetBidsResult::Failure(error) => {
                error!(
                    block_hash=?block.hash(),
                    ?state_root_hash,
                    ?error,
                    "failed to get bids"
                );
                return Err(Error::new(
                    ReservedErrorCode::InternalError,
                    format!(
                        "error getting bids at block {:?}: {:?}",
                        block.hash().inner(),
                        error
                    ),
                ));
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
            Err(error) => {
                error!(block_hash=?block.hash(), ?state_root_hash, ?error, "failed to get era validators");
                return Err(Error::new(
                    ReservedErrorCode::InternalError,
                    format!(
                        "failed to get validators at block {:?}: {}",
                        block.hash().inner(),
                        error
                    ),
                ));
            }
        };

        let auction_state = AuctionState::new(state_root_hash, block_height, era_validators, bids);

        let result = Self::ResponseResult {
            api_version,
            auction_state,
        };
        Ok(result)
    }
}

/// Identifier of an account.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[serde(deny_unknown_fields, untagged)]
pub enum AccountIdentifier {
    /// The public key of an account
    PublicKey(PublicKey),
    /// The account hash of an account
    AccountHash(AccountHash),
}

/// Params for "state_get_account_info" RPC request
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAccountInfoParams {
    /// The public key of the Account.
    #[serde(alias = "public_key")]
    pub account_identifier: AccountIdentifier,
    /// The block identifier.
    pub block_identifier: Option<BlockIdentifier>,
}

impl DocExample for GetAccountInfoParams {
    fn doc_example() -> &'static Self {
        &GET_ACCOUNT_INFO_PARAMS
    }
}

/// Result for "state_get_account_info" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAccountInfoResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The account.
    pub account: Account,
    /// The Merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetAccountInfoResult {
    fn doc_example() -> &'static Self {
        &GET_ACCOUNT_INFO_RESULT
    }
}

/// "state_get_account_info" RPC.
pub struct GetAccountInfo {}

#[async_trait]
impl RpcWithParams for GetAccountInfo {
    const METHOD: &'static str = "state_get_account_info";
    type RequestParams = GetAccountInfoParams;
    type ResponseResult = GetAccountInfoResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        // This RPC request is restricted by the block availability index.
        let only_from_available_block_range = true;

        let maybe_block_id = params.block_identifier;
        let block = common::get_block(
            maybe_block_id,
            only_from_available_block_range,
            effect_builder,
        )
        .await?;

        let state_root_hash = *block.state_root_hash();
        let base_key = {
            let account_hash = match params.account_identifier {
                AccountIdentifier::PublicKey(public_key) => public_key.to_account_hash(),
                AccountIdentifier::AccountHash(account_hash) => account_hash,
            };
            Key::Account(account_hash)
        };
        let (stored_value, merkle_proof) =
            common::run_query_and_encode(effect_builder, state_root_hash, base_key, vec![]).await?;

        let account = if let StoredValue::Account(account) = stored_value {
            account
        } else {
            let error_msg = format!("stored value is not an account for account {}", base_key);
            info!(?stored_value, "{}", error_msg);
            return Err(Error::new(ErrorCode::NoSuchAccount, error_msg));
        };

        let result = Self::ResponseResult {
            api_version,
            account,
            merkle_proof,
        };

        Ok(result)
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
    fn get_dictionary_address(
        &self,
        maybe_stored_value: Option<StoredValue>,
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
                    Some(StoredValue::Account(account)) => account.named_keys(),
                    Some(StoredValue::AddressableEntity(contract)) => contract.named_keys(),
                    Some(other) => {
                        return Err(Error::new(
                            ErrorCode::FailedToGetDictionaryURef,
                            format!(
                                "expected account or contract, but got {}",
                                other.type_name()
                            ),
                        ))
                    }
                    None => {
                        return Err(Error::new(
                            ErrorCode::FailedToGetDictionaryURef,
                            "could not retrieve account/contract".to_string(),
                        ))
                    }
                };

                let key_bytes = dictionary_item_key.as_str().as_bytes();
                let seed_uref = match named_keys.get(dictionary_name) {
                    Some(key) => *key.as_uref().ok_or_else(|| {
                        Error::new(
                            ErrorCode::FailedToGetDictionaryURef,
                            format!("expected uref, but got {}", key),
                        )
                    })?,
                    None => {
                        return Err(Error::new(
                            ErrorCode::FailedToGetDictionaryURef,
                            "seed uref not in named keys".to_string(),
                        ))
                    }
                };

                Ok(Key::dictionary(seed_uref, key_bytes))
            }
            DictionaryIdentifier::URef {
                seed_uref,
                dictionary_item_key,
            } => {
                let key_bytes = dictionary_item_key.as_str().as_bytes();
                let seed_uref = URef::from_formatted_str(seed_uref).map_err(|error| {
                    Error::new(
                        ErrorCode::FailedToGetDictionaryURef,
                        format!("failed to parse uref: {}", error),
                    )
                })?;
                Ok(Key::dictionary(seed_uref, key_bytes))
            }
            DictionaryIdentifier::Dictionary(address) => {
                Key::from_formatted_str(address).map_err(|error| {
                    Error::new(
                        ErrorCode::FailedToGetDictionaryURef,
                        format!("failed to parse dictionary key: {}", error),
                    )
                })
            }
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
        &GET_DICTIONARY_ITEM_PARAMS
    }
}

/// Result for "state_get_dictionary_item" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDictionaryItemResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The key under which the value is stored.
    pub dictionary_key: String,
    /// The stored value.
    pub stored_value: StoredValue,
    /// The Merkle proof.
    pub merkle_proof: String,
}

impl DocExample for GetDictionaryItemResult {
    fn doc_example() -> &'static Self {
        &GET_DICTIONARY_ITEM_RESULT
    }
}

/// "state_get_dictionary_item" RPC.
pub struct GetDictionaryItem {}

#[async_trait]
impl RpcWithParams for GetDictionaryItem {
    const METHOD: &'static str = "state_get_dictionary_item";
    type RequestParams = GetDictionaryItemParams;
    type ResponseResult = GetDictionaryItemResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let dictionary_query_key = match params.dictionary_identifier {
            DictionaryIdentifier::AccountNamedKey { ref key, .. }
            | DictionaryIdentifier::ContractNamedKey { ref key, .. } => {
                let base_key = match Key::from_formatted_str(key) {
                    Ok(key) => key,
                    Err(error) => {
                        return Err(Error::new(
                            ErrorCode::FailedToParseQueryKey,
                            format!("failed to parse base key: {}", error),
                        ))
                    }
                };

                let empty_path = Vec::new();
                let (value, _proofs) =
                    run_query(effect_builder, params.state_root_hash, base_key, empty_path).await?;
                params
                    .dictionary_identifier
                    .get_dictionary_address(Some(value))?
            }
            DictionaryIdentifier::URef { .. } | DictionaryIdentifier::Dictionary(_) => {
                params.dictionary_identifier.get_dictionary_address(None)?
            }
        };

        let (stored_value, merkle_proof) = common::run_query_and_encode(
            effect_builder,
            params.state_root_hash,
            dictionary_query_key,
            vec![],
        )
        .await?;

        let result = Self::ResponseResult {
            api_version,
            dictionary_key: dictionary_query_key.to_formatted_string(),
            stored_value,
            merkle_proof,
        };
        Ok(result)
    }
}

/// Identifier for possible ways to query Global State
#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub enum GlobalStateIdentifier {
    /// Query using a block hash.
    BlockHash(BlockHash),
    /// Query using a block height.
    BlockHeight(u64),
    /// Query using the state root hash.
    StateRootHash(Digest),
}

/// Params for "query_global_state" RPC
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryGlobalStateParams {
    /// The identifier used for the query. If not provided, the tip of the chain will be used.
    pub state_identifier: Option<GlobalStateIdentifier>,
    /// The key under which to query.
    pub key: Key,
    /// The path components starting from the key as base.
    #[serde(default)]
    pub path: Vec<String>,
}

impl DocExample for QueryGlobalStateParams {
    fn doc_example() -> &'static Self {
        &QUERY_GLOBAL_STATE_PARAMS
    }
}

/// Result for "query_global_state" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryGlobalStateResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The block header if a Block hash was provided.
    pub block_header: Option<BlockHeader>,
    /// The stored value.
    pub stored_value: StoredValue,
    /// The Merkle proof.
    pub merkle_proof: String,
}

impl DocExample for QueryGlobalStateResult {
    fn doc_example() -> &'static Self {
        &QUERY_GLOBAL_STATE_RESULT
    }
}

/// "query_global_state" RPC
pub struct QueryGlobalState {}

#[async_trait]
impl RpcWithParams for QueryGlobalState {
    const METHOD: &'static str = "query_global_state";
    type RequestParams = QueryGlobalStateParams;
    type ResponseResult = QueryGlobalStateResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let (state_root_hash, maybe_block_header) = match params.state_identifier {
            None => match effect_builder
                .get_highest_complete_block_header_from_storage()
                .await
            {
                None => {
                    return Err(Error::new(
                        ErrorCode::NoSuchBlock,
                        "query-global-state failed to retrieve highest block header",
                    ))
                }
                Some(block_header) => (*block_header.state_root_hash(), Some(block_header)),
            },
            Some(state_identifier) => {
                let (state_root_hash, maybe_block_header) =
                    get_state_root_hash_and_optional_header(effect_builder, state_identifier)
                        .await?;
                (state_root_hash, maybe_block_header)
            }
        };

        let (stored_value, merkle_proof) =
            common::run_query_and_encode(effect_builder, state_root_hash, params.key, params.path)
                .await?;

        let result = Self::ResponseResult {
            api_version,
            block_header: maybe_block_header,
            stored_value,
            merkle_proof,
        };
        Ok(result)
    }
}

/// Identifier of a purse.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum PurseIdentifier {
    /// The main purse of the account identified by this public key.
    MainPurseUnderPublicKey(PublicKey),
    /// The main purse of the account identified by this account hash.
    MainPurseUnderAccountHash(AccountHash),
    /// The purse identified by this URef.
    PurseUref(URef),
}

/// Params for "query_balance" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct QueryBalanceParams {
    /// The state identifier used for the query, if none is passed
    /// the tip of the chain will be used.
    pub state_identifier: Option<GlobalStateIdentifier>,
    /// The identifier to obtain the purse corresponding to balance query.
    pub purse_identifier: PurseIdentifier,
}

impl DocExample for QueryBalanceParams {
    fn doc_example() -> &'static Self {
        &QUERY_BALANCE_PARAMS
    }
}

/// Result for "query_balance" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
pub struct QueryBalanceResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The balance represented in motes.
    pub balance: U512,
}

impl DocExample for QueryBalanceResult {
    fn doc_example() -> &'static Self {
        &QUERY_BALANCE_RESULT
    }
}

/// "query_balance" RPC.
pub struct QueryBalance {}

#[async_trait]
impl RpcWithParams for QueryBalance {
    const METHOD: &'static str = "query_balance";
    type RequestParams = QueryBalanceParams;
    type ResponseResult = QueryBalanceResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let state_root_hash = match params.state_identifier {
            None => match effect_builder
                .get_highest_complete_block_header_from_storage()
                .await
            {
                None => {
                    return Err(Error::new(
                        ErrorCode::NoSuchBlock,
                        "query-balance failed to retrieve highest block header",
                    ))
                }
                Some(block_header) => *block_header.state_root_hash(),
            },
            Some(state_identifier) => {
                let (state_root_hash, _) =
                    get_state_root_hash_and_optional_header(effect_builder, state_identifier)
                        .await?;
                state_root_hash
            }
        };

        let purse_uref = match params.purse_identifier {
            PurseIdentifier::MainPurseUnderPublicKey(account_public_key) => {
                get_main_purse_by_account_hash(
                    effect_builder,
                    state_root_hash,
                    account_public_key.to_account_hash(),
                )
                .await?
            }
            PurseIdentifier::MainPurseUnderAccountHash(account_hash) => {
                get_main_purse_by_account_hash(effect_builder, state_root_hash, account_hash)
                    .await?
            }
            PurseIdentifier::PurseUref(purse_uref) => purse_uref,
        };

        // Get the balance.
        let balance_result = effect_builder
            .make_request(
                |responder| RpcRequest::GetBalance {
                    state_root_hash,
                    purse_uref,
                    responder,
                },
                QueueKind::Api,
            )
            .await;

        let balance_value = match balance_result {
            BalanceResult::Success { motes, .. } => motes,
            BalanceResult::RootNotFound => {
                info!(
                    %state_root_hash,
                    %purse_uref,
                    "query-balance failed: root not found"
                );
                return Err(Error::new(
                    ErrorCode::FailedToGetBalance,
                    format!(
                        "root hash {} not found when querying for purse {}",
                        state_root_hash, purse_uref
                    ),
                ));
            }
            BalanceResult::Failure(error) => {
                info!("query-balance failed to execute: {}", error);
                return Err(Error::new(
                    ErrorCode::GetBalanceFailedToExecute,
                    error.to_string(),
                ));
            }
        };

        let result = Self::ResponseResult {
            api_version,
            balance: balance_value,
        };
        Ok(result)
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
        &GET_TRIE_PARAMS
    }
}

/// Result for "state_get_trie" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
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
        &GET_TRIE_RESULT
    }
}

/// `state_get_trie` RPC.
pub struct GetTrie {}

#[async_trait]
impl RpcWithParams for GetTrie {
    const METHOD: &'static str = "state_get_trie";
    type RequestParams = GetTrieParams;
    type ResponseResult = GetTrieResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let trie_key = params.trie_key;

        match effect_builder.get_trie_full(trie_key).await {
            Ok(maybe_trie_bytes) => {
                let result = Self::ResponseResult {
                    api_version,
                    maybe_trie_bytes,
                };
                Ok(result)
            }
            Err(error) => {
                warn!(?error, "failed to get trie");
                Err(Error::new(
                    ErrorCode::FailedToGetTrie,
                    format!("{:?}", error),
                ))
            }
        }
    }
}

type QuerySuccess = (StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>);

/// Runs a global state query and returns a tuple of the domain stored value and Merkle proof of the
/// value.
///
/// On error, a `warp_json_rpc::Error` is returned suitable for sending as a JSON-RPC response.
pub(super) async fn run_query<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    base_key: Key,
    path: Vec<String>,
) -> Result<QuerySuccess, Error> {
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

    handle_query_result(effect_builder, state_root_hash, query_result).await
}

/// Converts the result of running a global state query into a type suitable for including in a
/// JSON-RPC response.
pub(super) async fn handle_query_result<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    query_result: QueryResult,
) -> Result<QuerySuccess, Error> {
    if let QueryResult::RootNotFound = query_result {
        info!("query failed: root not found");
        let error = common::missing_block_or_state_root_error(
            effect_builder,
            ErrorCode::NoSuchStateRoot,
            format!("failed to get state root at {:?}", state_root_hash),
        )
        .await;
        return Err(error);
    }

    if let QueryResult::Success { value, proofs } = query_result {
        return Ok((*value, proofs));
    }

    debug!(?query_result, "query failed");
    Err(Error::new(
        ErrorCode::QueryFailed,
        format!("{:?}", query_result),
    ))
}

async fn get_main_purse_by_account_hash<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    account_hash: AccountHash,
) -> Result<URef, Error> {
    let (stored_value, _) = common::run_query_and_encode(
        effect_builder,
        state_root_hash,
        Key::Account(account_hash),
        vec![],
    )
    .await?;

    let error = {
        let error_msg = format!("failed to get main purse for {}", account_hash);
        info!(?stored_value, "{}", error_msg);
        Err(Error::new(ErrorCode::NoSuchMainPurse, error_msg))
    };

    match stored_value {
        StoredValue::Account(account) => Ok(account.main_purse()),
        StoredValue::CLValue(entity_key_as_clvalue) => {
            let entity_key: Key = match CLValue::into_t(entity_key_as_clvalue) {
                Ok(entity_key) => entity_key,
                Err(_) => return error,
            };
            let (entity_value, _) =
                common::run_query_and_encode(effect_builder, state_root_hash, entity_key, vec![])
                    .await?;
            if let StoredValue::AddressableEntity(entity) = entity_value {
                Ok(entity.main_purse())
            } else {
                error
            }
        }
        _ => error,
    }
}

pub(super) async fn get_state_root_hash_and_optional_header<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    state_identifier: GlobalStateIdentifier,
) -> Result<(Digest, Option<BlockHeader>), Error> {
    // This RPC request is restricted by the block availability index.
    let only_from_available_block_range = true;
    match state_identifier {
        GlobalStateIdentifier::BlockHash(block_hash) => {
            match effect_builder
                .get_block_header_from_storage(block_hash, only_from_available_block_range)
                .await
            {
                None => {
                    let error_msg =
                        format!("failed to retrieve specified block header {}", block_hash);
                    Err(Error::new(ErrorCode::NoSuchBlock, error_msg))
                }
                Some(block_header) => Ok((*block_header.state_root_hash(), Some(block_header))),
            }
        }
        GlobalStateIdentifier::BlockHeight(block_height) => {
            match effect_builder
                .get_block_header_at_height_from_storage(
                    block_height,
                    only_from_available_block_range,
                )
                .await
            {
                None => {
                    let error_msg =
                        format!("failed to retrieve block header at height {}", block_height);
                    Err(Error::new(ErrorCode::NoSuchBlock, error_msg))
                }
                Some(block_header) => Ok((*block_header.state_root_hash(), Some(block_header))),
            }
        }
        GlobalStateIdentifier::StateRootHash(state_root_hash) => Ok((state_root_hash, None)),
    }
}
