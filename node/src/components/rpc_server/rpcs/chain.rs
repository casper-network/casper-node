//! RPCs related to the block chain.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

mod era_summary;

use std::{num::ParseIntError, str};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use warp_json_rpc::Builder;

use casper_hashing::Digest;
use casper_types::{Key, ProtocolVersion, Transfer};

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithOptionalParams, RpcWithOptionalParamsExt,
};
use crate::{
    effect::EffectBuilder,
    reactor::QueueKind,
    rpcs::common,
    types::{Block, BlockHash, BlockWithMetadata, JsonBlock},
};
pub use era_summary::EraSummary;
use era_summary::ERA_SUMMARY;

static GET_BLOCK_PARAMS: Lazy<GetBlockParams> = Lazy::new(|| GetBlockParams {
    block_identifier: BlockIdentifier::Hash(*Block::doc_example().hash()),
});
static GET_BLOCK_RESULT: Lazy<GetBlockResult> = Lazy::new(|| GetBlockResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    block: Some(JsonBlock::doc_example().clone()),
});
static GET_BLOCK_TRANSFERS_PARAMS: Lazy<GetBlockTransfersParams> =
    Lazy::new(|| GetBlockTransfersParams {
        block_identifier: BlockIdentifier::Hash(*Block::doc_example().hash()),
    });
static GET_BLOCK_TRANSFERS_RESULT: Lazy<GetBlockTransfersResult> =
    Lazy::new(|| GetBlockTransfersResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        block_hash: Some(*Block::doc_example().hash()),
        transfers: Some(vec![Transfer::default()]),
    });
static GET_STATE_ROOT_HASH_PARAMS: Lazy<GetStateRootHashParams> =
    Lazy::new(|| GetStateRootHashParams {
        block_identifier: BlockIdentifier::Height(Block::doc_example().header().height()),
    });
static GET_STATE_ROOT_HASH_RESULT: Lazy<GetStateRootHashResult> =
    Lazy::new(|| GetStateRootHashResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        state_root_hash: Some(*Block::doc_example().header().state_root_hash()),
    });
static GET_ERA_INFO_PARAMS: Lazy<GetEraInfoParams> = Lazy::new(|| GetEraInfoParams {
    block_identifier: BlockIdentifier::Hash(*Block::doc_example().hash()),
});
static GET_ERA_INFO_RESULT: Lazy<GetEraInfoResult> = Lazy::new(|| GetEraInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    era_summary: Some(ERA_SUMMARY.clone()),
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
    FromHexError(casper_hashing::Error),
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
        &*GET_BLOCK_PARAMS
    }
}

/// Result for "chain_get_block" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
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
        &*GET_BLOCK_RESULT
    }
}

/// "chain_get_block" RPC.
pub struct GetBlock {}

impl RpcWithOptionalParams for GetBlock {
    const METHOD: &'static str = "chain_get_block";
    type OptionalRequestParams = GetBlockParams;
    type ResponseResult = GetBlockResult;
}

impl RpcWithOptionalParamsExt for GetBlock {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // This RPC request is restricted by the block availability index.
            let only_from_available_block_range = true;

            // Get the block.
            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
            let json_block = match get_block_with_metadata(
                maybe_block_id,
                only_from_available_block_range,
                effect_builder,
            )
            .await
            {
                Ok(BlockWithMetadata {
                    block,
                    finality_signatures,
                }) => JsonBlock::new(block, Some(finality_signatures)),
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let result = Self::ResponseResult {
                api_version,
                block: Some(json_block),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
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
        &*GET_BLOCK_TRANSFERS_PARAMS
    }
}

/// Result for "chain_get_block_transfers" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
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
        &*GET_BLOCK_TRANSFERS_RESULT
    }
}

/// "chain_get_block_transfers" RPC.
pub struct GetBlockTransfers {}

impl RpcWithOptionalParams for GetBlockTransfers {
    const METHOD: &'static str = "chain_get_block_transfers";
    type OptionalRequestParams = GetBlockTransfersParams;
    type ResponseResult = GetBlockTransfersResult;
}

impl RpcWithOptionalParamsExt for GetBlockTransfers {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // This RPC request is restricted by the block availability index.
            let only_from_available_block_range = true;

            // Get the block.
            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
            let block_hash = match common::get_block(
                maybe_block_id,
                only_from_available_block_range,
                effect_builder,
            )
            .await
            {
                Ok(block) => *block.hash(),
                Err(error) => return Ok(response_builder.error(error)?),
            };

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
            Ok(response_builder.success(result)?)
        }
        .boxed()
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
        &*GET_STATE_ROOT_HASH_PARAMS
    }
}

/// Result for "chain_get_state_root_hash" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
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
        &*GET_STATE_ROOT_HASH_RESULT
    }
}

/// "chain_get_state_root_hash" RPC.
pub struct GetStateRootHash {}

impl RpcWithOptionalParams for GetStateRootHash {
    const METHOD: &'static str = "chain_get_state_root_hash";
    type OptionalRequestParams = GetStateRootHashParams;
    type ResponseResult = GetStateRootHashResult;
}

impl RpcWithOptionalParamsExt for GetStateRootHash {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // This RPC request is restricted by the block availability index.
            let only_from_available_block_range = true;

            // Get the block.
            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
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

            // Return the result.
            let result = Self::ResponseResult {
                api_version,
                state_root_hash: Some(*block.state_root_hash()),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
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
        &*GET_ERA_INFO_PARAMS
    }
}

/// Result for "chain_get_era_info" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
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
        &*GET_ERA_INFO_RESULT
    }
}

/// "chain_get_era_info_by_switch_block" RPC
pub struct GetEraInfoBySwitchBlock {}

impl RpcWithOptionalParams for GetEraInfoBySwitchBlock {
    const METHOD: &'static str = "chain_get_era_info_by_switch_block";
    type OptionalRequestParams = GetEraInfoParams;
    type ResponseResult = GetEraInfoResult;
}

impl RpcWithOptionalParamsExt for GetEraInfoBySwitchBlock {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // This RPC request is restricted by the block availability index.
            let only_from_available_block_range = true;

            // TODO: decide if/how to handle era id
            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
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

            if !block.header().is_switch_block() {
                return Ok(response_builder.success(Self::ResponseResult {
                    api_version,
                    era_summary: None,
                })?);
            }

            let state_root_hash = block.state_root_hash().to_owned();
            let era_id = block.header().era_id();
            let base_key = Key::EraInfo(era_id);
            let path = Vec::new();

            let query_result =
                common::run_query_and_encode(effect_builder, state_root_hash, base_key, path).await;

            match query_result {
                Ok((stored_value, merkle_proof)) => {
                    let result = Self::ResponseResult {
                        api_version,
                        era_summary: Some(EraSummary {
                            block_hash: *block.hash(),
                            era_id,
                            stored_value,
                            state_root_hash,
                            merkle_proof,
                        }),
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => Ok(response_builder.error(error)?),
            }
        }
        .boxed()
    }
}

pub(super) async fn get_block_with_metadata<REv: ReactorEventT>(
    maybe_id: Option<BlockIdentifier>,
    only_from_available_block_range: bool,
    effect_builder: EffectBuilder<REv>,
) -> Result<BlockWithMetadata, warp_json_rpc::Error> {
    // Get the block from storage or the latest from the linear chain.
    let maybe_result = effect_builder
        .make_request(
            |responder| RpcRequest::GetBlock {
                maybe_id,
                only_from_available_block_range,
                responder,
            },
            QueueKind::Api,
        )
        .await;

    if let Some(block_with_metadata) = maybe_result {
        return Ok(block_with_metadata);
    }

    // TODO: Potential optimization: We might want to make the `GetBlock` actually return the
    //       available block range, so we don't need to request it again inside the
    //       `missing_block_or_state_root_error` function.
    let error = match maybe_id {
        Some(BlockIdentifier::Hash(block_hash)) => common::missing_block_or_state_root_error(
            effect_builder,
            ErrorCode::NoSuchBlock,
            format!("block {:?} not stored on this node", block_hash.inner()),
        ),
        Some(BlockIdentifier::Height(block_height)) => common::missing_block_or_state_root_error(
            effect_builder,
            ErrorCode::NoSuchBlock,
            format!("block at height {} not stored on this node", block_height),
        ),
        None => common::missing_block_or_state_root_error(
            effect_builder,
            ErrorCode::InternalError,
            "failed to get highest block".to_string(),
        ),
    }
    .await;

    Err(error)
}
