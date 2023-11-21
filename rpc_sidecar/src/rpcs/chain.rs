//! RPCs related to the block chain.

mod era_summary;

use std::{clone::Clone, num::ParseIntError, str, sync::Arc};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{
    binary_port::global_state::GlobalStateQueryResult,
    Block, BlockHash, BlockHeaderV2, Digest, DigestError,
    JsonBlockWithSignatures, Key, ProtocolVersion, StoredValue, Transfer,
};

use crate::NodeClient;

use super::{
    common,
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error as RpcError, RpcWithOptionalParams,
};
use crate::rpcs::error::Error;
pub use era_summary::EraSummary;
use era_summary::ERA_SUMMARY;

static GET_BLOCK_PARAMS: Lazy<GetBlockParams> = Lazy::new(|| GetBlockParams {
    block_identifier: BlockIdentifier::Hash(*JsonBlockWithSignatures::example().block.hash()),
});
static GET_BLOCK_RESULT: Lazy<GetBlockResult> = Lazy::new(|| GetBlockResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    block_with_signatures: Some(JsonBlockWithSignatures::example().clone()),
});
static GET_BLOCK_TRANSFERS_PARAMS: Lazy<GetBlockTransfersParams> =
    Lazy::new(|| GetBlockTransfersParams {
        block_identifier: BlockIdentifier::Hash(*BlockHash::example()),
    });
static GET_BLOCK_TRANSFERS_RESULT: Lazy<GetBlockTransfersResult> =
    Lazy::new(|| GetBlockTransfersResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        block_hash: Some(*BlockHash::example()),
        transfers: Some(vec![Transfer::default()]),
    });
static GET_STATE_ROOT_HASH_PARAMS: Lazy<GetStateRootHashParams> =
    Lazy::new(|| GetStateRootHashParams {
        block_identifier: BlockIdentifier::Height(BlockHeaderV2::example().height()),
    });
static GET_STATE_ROOT_HASH_RESULT: Lazy<GetStateRootHashResult> =
    Lazy::new(|| GetStateRootHashResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        state_root_hash: Some(*BlockHeaderV2::example().state_root_hash()),
    });
static GET_ERA_INFO_PARAMS: Lazy<GetEraInfoParams> = Lazy::new(|| GetEraInfoParams {
    block_identifier: BlockIdentifier::Hash(ERA_SUMMARY.block_hash),
});
static GET_ERA_INFO_RESULT: Lazy<GetEraInfoResult> = Lazy::new(|| GetEraInfoResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    era_summary: Some(ERA_SUMMARY.clone()),
});
static GET_ERA_SUMMARY_PARAMS: Lazy<GetEraSummaryParams> = Lazy::new(|| GetEraSummaryParams {
    block_identifier: BlockIdentifier::Hash(ERA_SUMMARY.block_hash),
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
    pub block_with_signatures: Option<JsonBlockWithSignatures>,
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

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let identifier = maybe_params.map(|params| params.block_identifier);
        let (block, signatures) = common::get_signed_block(&*node_client, identifier)
            .await?
            .into_inner();
        Ok(Self::ResponseResult {
            api_version,
            block_with_signatures: Some(JsonBlockWithSignatures::new(block, Some(signatures))),
        })
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
    // TODO: will be used
    #[allow(unused)]
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

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let identifier = maybe_params.map(|params| params.block_identifier);
        let block = common::get_signed_block(&*node_client, identifier).await?;
        let transfers = node_client
            .read_block_transfers(*block.block().hash())
            .await
            .map_err(|err| Error::NodeRequest("block transfers", err))?;
        Ok(Self::ResponseResult {
            api_version,
            block_hash: Some(*block.block().hash()),
            transfers,
        })
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

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let identifier = maybe_params.map(|params| params.block_identifier);
        let (block, _) = common::get_signed_block(&*node_client, identifier)
            .await?
            .into_inner();
        Ok(Self::ResponseResult {
            api_version,
            state_root_hash: Some(*block.state_root_hash()),
        })
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

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let identifier = maybe_params.map(|params| params.block_identifier);
        let (block, _) = common::get_signed_block(&*node_client, identifier)
            .await?
            .into_inner();
        let era_summary = if block.is_switch_block() {
            Some(get_era_summary_by_block(node_client, &block).await?)
        } else {
            None
        };

        Ok(Self::ResponseResult {
            api_version,
            era_summary,
        })
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

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        api_version: ProtocolVersion,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let identifier = maybe_params.map(|params| params.block_identifier);
        let (block, _) = common::get_signed_block(&*node_client, identifier)
            .await?
            .into_inner();
        let era_summary = get_era_summary_by_block(node_client, &block).await?;

        Ok(Self::ResponseResult {
            api_version,
            era_summary,
        })
    }
}

async fn get_era_summary_by_block(
    node_client: Arc<dyn NodeClient>,
    block: &Block,
) -> Result<EraSummary, Error> {
    fn create_era_summary(
        block: &Block,
        stored_value: StoredValue,
        merkle_proof: String,
    ) -> EraSummary {
        EraSummary {
            block_hash: *block.hash(),
            era_id: block.era_id(),
            stored_value,
            state_root_hash: *block.state_root_hash(),
            merkle_proof,
        }
    }

    let state_root_hash = *block.state_root_hash();
    let era_summary = node_client
        .query_global_state(state_root_hash, Key::EraSummary, vec![])
        .await
        .map_err(|err| Error::NodeRequest("era summary", err))?;

    let era_summary = if !matches!(era_summary, GlobalStateQueryResult::ValueNotFound) {
        let era_summary = common::handle_query_result(era_summary)?;
        create_era_summary(&block, era_summary.value, era_summary.merkle_proof)
    } else {
        let era_info = node_client
            .query_global_state(state_root_hash, Key::EraInfo(block.era_id()), vec![])
            .await
            .map_err(|err| Error::NodeRequest("era info", err))?;
        let era_info = common::handle_query_result(era_info)?;
        create_era_summary(&block, era_info.value, era_info.merkle_proof)
    };
    Ok(era_summary)
}
