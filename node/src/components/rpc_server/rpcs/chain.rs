//! RPCs related to the block chain.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

mod era_summary;

use std::{clone::Clone, num::ParseIntError, str};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_execution_engine::engine_state::{self, QueryResult};
use casper_types::{
    Block, BlockHash, BlockV2, Digest, DigestError, JsonBlock, Key, ProtocolVersion, Transfer,
};

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, ReservedErrorCode, RpcRequest, RpcWithOptionalParams,
};
use crate::{
    effect::EffectBuilder,
    reactor::QueueKind,
    rpcs::{common, state},
    types::SignedBlock,
};
pub use era_summary::EraSummary;
use era_summary::ERA_SUMMARY;

static GET_BLOCK_PARAMS: Lazy<GetBlockParams> = Lazy::new(|| GetBlockParams {
    block_identifier: BlockIdentifier::Hash(JsonBlock::doc_example().hash),
});
static GET_BLOCK_RESULT: Lazy<GetBlockResult> = Lazy::new(|| GetBlockResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    block: Some(JsonBlock::doc_example().clone()),
});
static GET_BLOCK_TRANSFERS_PARAMS: Lazy<GetBlockTransfersParams> =
    Lazy::new(|| GetBlockTransfersParams {
        block_identifier: BlockIdentifier::Hash(JsonBlock::doc_example().hash),
    });
static GET_BLOCK_TRANSFERS_RESULT: Lazy<GetBlockTransfersResult> =
    Lazy::new(|| GetBlockTransfersResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        block_hash: Some(JsonBlock::doc_example().hash),
        transfers: Some(vec![Transfer::default()]),
    });
static GET_STATE_ROOT_HASH_PARAMS: Lazy<GetStateRootHashParams> =
    Lazy::new(|| GetStateRootHashParams {
        block_identifier: BlockIdentifier::Height(JsonBlock::doc_example().header.height),
    });
static GET_STATE_ROOT_HASH_RESULT: Lazy<GetStateRootHashResult> =
    Lazy::new(|| GetStateRootHashResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        state_root_hash: Some(JsonBlock::doc_example().header.state_root_hash),
    });
static GET_ERA_INFO_PARAMS: Lazy<GetEraInfoParams> = Lazy::new(|| GetEraInfoParams {
    block_identifier: BlockIdentifier::Hash(JsonBlock::doc_example().hash),
});
static GET_ERA_INFO_RESULT: Lazy<GetEraInfoResult> = Lazy::new(|| GetEraInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    era_summary: Some(ERA_SUMMARY.clone()),
});
static GET_ERA_SUMMARY_PARAMS: Lazy<GetEraSummaryParams> = Lazy::new(|| GetEraSummaryParams {
    block_identifier: BlockIdentifier::Hash(JsonBlock::doc_example().hash),
});
static GET_ERA_SUMMARY_RESULT: Lazy<GetEraSummaryResult> = Lazy::new(|| GetEraSummaryResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    era_summary: ERA_SUMMARY.clone(),
});

/// Identifier for possible ways to retrieve a block.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum BlockIdentifier {
    /// Identify and retrieve the block with its hash.
    Hash(BlockHash),
    /// Identify and retrieve the block with its height.
    Height(u64),
}

impl str::FromStr for BlockIdentifier {
    type Err = ParseBlockIdentifierError;

    fn from_str(maybe_block_identifier: &str) -> Result<Self, Self::Err> {
        if maybe_block_identifier.is_empty() {
            return Err(ParseBlockIdentifierError::EmptyString);
        }

        if maybe_block_identifier.len() == (Digest::LENGTH * 2) {
            let hash = Digest::from_hex(maybe_block_identifier)
                .map_err(ParseBlockIdentifierError::FromHexError)?;
            Ok(BlockIdentifier::Hash(BlockHash::new(hash)))
        } else {
            let height = maybe_block_identifier
                .parse()
                .map_err(ParseBlockIdentifierError::ParseIntError)?;
            Ok(BlockIdentifier::Height(height))
        }
    }
}

/// Represents errors that can arise when parsing a [`BlockIdentifier`].
#[derive(thiserror::Error, Debug)]
pub enum ParseBlockIdentifierError {
    /// String was empty.
    #[error("Empty string is not a valid block identifier.")]
    EmptyString,
    /// Couldn't parse a height value.
    #[error("Unable to parse height from string. {0}")]
    ParseIntError(ParseIntError),
    /// Couldn't parse a blake2bhash.
    #[error("Unable to parse digest from string. {0}")]
    FromHexError(DigestError),
}

/// Params for "chain_get_block" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBlockParams {
    /// The block identifier.
    pub block_identifier: BlockIdentifier,
}

impl DocExample for GetBlockParams {
    fn doc_example() -> &'static Self {
        &GET_BLOCK_PARAMS
    }
}

/// Result for "chain_get_block" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBlockResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The block, if found.
    pub block: Option<JsonBlock>,
}

impl DocExample for GetBlockResult {
    fn doc_example() -> &'static Self {
        &GET_BLOCK_RESULT
    }
}

/// "chain_get_block" RPC.
pub struct GetBlock {}

#[async_trait]
impl RpcWithOptionalParams for GetBlock {
    const METHOD: &'static str = "chain_get_block";
    type OptionalRequestParams = GetBlockParams;
    type ResponseResult = GetBlockResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, Error> {
        // This RPC request is restricted by the block availability index.
        let only_from_available_block_range = true;

        // Get the block.
        let maybe_block_id = maybe_params.map(|params| params.block_identifier);
        let SignedBlock {
            block,
            block_signatures,
        } = get_signed_block(
            maybe_block_id,
            only_from_available_block_range,
            effect_builder,
        )
        .await?;

        let block: BlockV2 = block.into(); // TODO: change this to support versioning
        let json_block = JsonBlock::new(block, Some(block_signatures));

        // Return the result.
        let result = Self::ResponseResult {
            api_version,
            block: Some(json_block),
        };
        Ok(result)
    }
}

/// Params for "chain_get_block_transfers" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBlockTransfersParams {
    /// The block hash.
    pub block_identifier: BlockIdentifier,
}

impl DocExample for GetBlockTransfersParams {
    fn doc_example() -> &'static Self {
        &GET_BLOCK_TRANSFERS_PARAMS
    }
}

/// Result for "chain_get_block_transfers" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBlockTransfersResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The block hash, if found.
    pub block_hash: Option<BlockHash>,
    /// The block's transfers, if found.
    pub transfers: Option<Vec<Transfer>>,
}

impl GetBlockTransfersResult {
    /// Create an instance of GetBlockTransfersResult.
    pub fn new(
        api_version: ProtocolVersion,
        block_hash: Option<BlockHash>,
        transfers: Option<Vec<Transfer>>,
    ) -> Self {
        GetBlockTransfersResult {
            api_version,
            block_hash,
            transfers,
        }
    }
}

impl DocExample for GetBlockTransfersResult {
    fn doc_example() -> &'static Self {
        &GET_BLOCK_TRANSFERS_RESULT
    }
}

/// "chain_get_block_transfers" RPC.
pub struct GetBlockTransfers {}

#[async_trait]
impl RpcWithOptionalParams for GetBlockTransfers {
    const METHOD: &'static str = "chain_get_block_transfers";
    type OptionalRequestParams = GetBlockTransfersParams;
    type ResponseResult = GetBlockTransfersResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, Error> {
        // This RPC request is restricted by the block availability index.
        let only_from_available_block_range = true;

        // Get the block.
        let maybe_block_id = maybe_params.map(|params| params.block_identifier);
        let block_hash = common::get_block(
            maybe_block_id,
            only_from_available_block_range,
            effect_builder,
        )
        .await
        .map(|block| *block.hash())?;

        let transfers = effect_builder
            .make_request(
                |responder| RpcRequest::GetBlockTransfers {
                    block_hash,
                    responder,
                },
                QueueKind::Api,
            )
            .await;

        // Return the result.
        let result = Self::ResponseResult::new(api_version, Some(block_hash), transfers);
        Ok(result)
    }
}

/// Params for "chain_get_state_root_hash" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStateRootHashParams {
    /// The block hash.
    pub block_identifier: BlockIdentifier,
}

impl DocExample for GetStateRootHashParams {
    fn doc_example() -> &'static Self {
        &GET_STATE_ROOT_HASH_PARAMS
    }
}

/// Result for "chain_get_state_root_hash" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStateRootHashResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// Hex-encoded hash of the state root.
    pub state_root_hash: Option<Digest>,
}

impl DocExample for GetStateRootHashResult {
    fn doc_example() -> &'static Self {
        &GET_STATE_ROOT_HASH_RESULT
    }
}

/// "chain_get_state_root_hash" RPC.
pub struct GetStateRootHash {}

#[async_trait]
impl RpcWithOptionalParams for GetStateRootHash {
    const METHOD: &'static str = "chain_get_state_root_hash";
    type OptionalRequestParams = GetStateRootHashParams;
    type ResponseResult = GetStateRootHashResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, Error> {
        // This RPC request is restricted by the block availability index.
        let only_from_available_block_range = true;

        // Get the block.
        let maybe_block_id = maybe_params.map(|params| params.block_identifier);
        let block = common::get_block(
            maybe_block_id,
            only_from_available_block_range,
            effect_builder,
        )
        .await?;

        // Return the result.
        let result = Self::ResponseResult {
            api_version,
            state_root_hash: Some(*block.state_root_hash()),
        };
        Ok(result)
    }
}

/// Params for "chain_get_era_info" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEraInfoParams {
    /// The block identifier.
    pub block_identifier: BlockIdentifier,
}

impl DocExample for GetEraInfoParams {
    fn doc_example() -> &'static Self {
        &GET_ERA_INFO_PARAMS
    }
}

/// Result for "chain_get_era_info" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEraInfoResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The era summary.
    pub era_summary: Option<EraSummary>,
}

impl DocExample for GetEraInfoResult {
    fn doc_example() -> &'static Self {
        &GET_ERA_INFO_RESULT
    }
}

/// "chain_get_era_info_by_switch_block" RPC
pub struct GetEraInfoBySwitchBlock {}

#[async_trait]
impl RpcWithOptionalParams for GetEraInfoBySwitchBlock {
    const METHOD: &'static str = "chain_get_era_info_by_switch_block";
    type OptionalRequestParams = GetEraInfoParams;
    type ResponseResult = GetEraInfoResult;

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

        if !block.is_switch_block() {
            return Ok(Self::ResponseResult {
                api_version,
                era_summary: None,
            });
        }

        let block: BlockV2 = block.into(); //TODO: change this to support versioning
        let era_summary = get_era_summary(effect_builder, &block).await?;
        let result = Self::ResponseResult {
            api_version,
            era_summary: Some(era_summary),
        };
        Ok(result)
    }
}

/// Params for "chain_get_era_summary" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEraSummaryParams {
    /// The block identifier.
    pub block_identifier: BlockIdentifier,
}

impl DocExample for GetEraSummaryParams {
    fn doc_example() -> &'static Self {
        &GET_ERA_SUMMARY_PARAMS
    }
}

/// Result for "chain_get_era_summary" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEraSummaryResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The era summary.
    pub era_summary: EraSummary,
}

impl DocExample for GetEraSummaryResult {
    fn doc_example() -> &'static Self {
        &GET_ERA_SUMMARY_RESULT
    }
}

/// "chain_get_era_summary" RPC
pub struct GetEraSummary {}

#[async_trait]
impl RpcWithOptionalParams for GetEraSummary {
    const METHOD: &'static str = "chain_get_era_summary";
    type OptionalRequestParams = GetEraSummaryParams;
    type ResponseResult = GetEraSummaryResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, Error> {
        let maybe_block_id = maybe_params.map(|params| params.block_identifier);
        let block = common::get_block(maybe_block_id, true, effect_builder).await?;

        let block: BlockV2 = block.into(); // TODO: change this to support versioning
        let era_summary = get_era_summary(effect_builder, &block).await?;
        let result = Self::ResponseResult {
            api_version,
            era_summary,
        };
        Ok(result)
    }
}

pub(super) async fn get_signed_block<REv: ReactorEventT>(
    maybe_id: Option<BlockIdentifier>,
    only_from_available_block_range: bool,
    effect_builder: EffectBuilder<REv>,
) -> Result<SignedBlock, Error> {
    let maybe_result = match maybe_id {
        Some(BlockIdentifier::Hash(hash)) => {
            effect_builder
                .get_signed_block_from_storage(hash, only_from_available_block_range)
                .await
        }
        Some(BlockIdentifier::Height(height)) => {
            effect_builder
                .get_signed_block_at_height_from_storage(height, only_from_available_block_range)
                .await
        }
        None => {
            effect_builder
                .get_highest_signed_block_from_storage(only_from_available_block_range)
                .await
        }
    };

    if let Some(signed_block) = maybe_result {
        return Ok(signed_block);
    }

    // TODO: Potential optimization: We might want to make the `GetBlock` actually return the
    //       available block range, so we don't need to request it again inside the
    //       `missing_block_or_state_root_error` function.
    let error = match maybe_id {
        Some(BlockIdentifier::Hash(block_hash)) => {
            common::missing_block_or_state_root_error(
                effect_builder,
                ErrorCode::NoSuchBlock,
                format!("block {:?} not stored on this node", block_hash.inner()),
            )
            .await
        }
        Some(BlockIdentifier::Height(block_height)) => {
            common::missing_block_or_state_root_error(
                effect_builder,
                ErrorCode::NoSuchBlock,
                format!("block at height {} not stored on this node", block_height),
            )
            .await
        }
        None => {
            common::missing_block_or_state_root_error(
                effect_builder,
                ReservedErrorCode::InternalError,
                "failed to get highest block".to_string(),
            )
            .await
        }
    };

    Err(error)
}

/// Returns the `EraSummary` for the era specified in the block.
///
/// Prior to Casper Mainnet version 1.4.15, era summaries were stored under `Key::EraInfo(era_id)`.
/// At this version and later, they are stored under `Key::EraSummary`.
///
/// As this change in behaviour is not related to a consistent protocol version across all Casper
/// networks, we simply try to find the `EraSummary` under the new key variant, and fall back to
/// to the old variant if the former executes correctly but fails to find the value.
async fn get_era_summary<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    block: &Block,
) -> Result<EraSummary, Error> {
    async fn handle_query_result<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        block: &Block,
        result: Result<QueryResult, engine_state::Error>,
    ) -> Result<EraSummary, Error> {
        let (value, proofs) =
            state::handle_query_result(effect_builder, *block.state_root_hash(), result).await?;
        let (stored_value, merkle_proof) = common::encode_query_success(value, proofs)?;
        Ok(EraSummary {
            block_hash: *block.hash(),
            era_id: block.era_id(),
            stored_value,
            state_root_hash: *block.state_root_hash(),
            merkle_proof,
        })
    }

    let era_summary_query_result = effect_builder
        .make_request(
            |responder| RpcRequest::QueryGlobalState {
                state_root_hash: *block.state_root_hash(),
                base_key: Key::EraSummary,
                path: vec![],
                responder,
            },
            QueueKind::Api,
        )
        .await;
    if !matches!(era_summary_query_result, Ok(QueryResult::ValueNotFound(_))) {
        // The query succeeded or failed in a way not requiring trying under `Key::EraInfo`.
        return handle_query_result(effect_builder, block, era_summary_query_result).await;
    }

    let era_info_query_result = effect_builder
        .make_request(
            |responder| RpcRequest::QueryGlobalState {
                state_root_hash: *block.state_root_hash(),
                base_key: Key::EraInfo(block.era_id()),
                path: vec![],
                responder,
            },
            QueueKind::Api,
        )
        .await;
    handle_query_result(effect_builder, block, era_info_query_result).await
}
