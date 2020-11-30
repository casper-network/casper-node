//! RPCs related to the block chain.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use super::{
    docs::DocExample, Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithOptionalParams,
    RpcWithOptionalParamsExt,
};
use crate::{
    components::CLIENT_API_VERSION,
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Block, BlockHash, Item, JsonBlock},
};

static GET_BLOCK_PARAMS: Lazy<GetBlockParams> = Lazy::new(|| GetBlockParams {
    block_identifier: BlockIdentifier::Hash(Block::doc_example().id()),
});
static GET_BLOCK_RESULT: Lazy<GetBlockResult> = Lazy::new(|| GetBlockResult {
    api_version: CLIENT_API_VERSION.clone(),
    block: Some(Block::doc_example().clone().into()),
});
static GET_STATE_ROOT_HASH_PARAMS: Lazy<GetStateRootHashParams> =
    Lazy::new(|| GetStateRootHashParams {
        block_identifier: BlockIdentifier::Height(Block::doc_example().header().height()),
    });
static GET_STATE_ROOT_HASH_RESULT: Lazy<GetStateRootHashResult> =
    Lazy::new(|| GetStateRootHashResult {
        api_version: CLIENT_API_VERSION.clone(),
        state_root_hash: Some(*Block::doc_example().header().state_root_hash()),
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

/// Params for "chain_get_block" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetBlockParams {
    /// The block hash.
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
    pub api_version: Version,
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
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Get the block.
            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
            let maybe_block = match get_block(maybe_block_id, effect_builder).await {
                Ok(maybe_block) => maybe_block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                block: maybe_block.map(Into::into),
            };
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
    pub api_version: Version,
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
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Get the block.
            let maybe_block_id = maybe_params.map(|params| params.block_identifier);
            let maybe_block = match get_block(maybe_block_id, effect_builder).await {
                Ok(maybe_block) => maybe_block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                state_root_hash: maybe_block.map(|block| *block.state_root_hash()),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

async fn get_block<REv: ReactorEventT>(
    maybe_id: Option<BlockIdentifier>,
    effect_builder: EffectBuilder<REv>,
) -> Result<Option<Block>, warp_json_rpc::Error> {
    // Get the block from storage or the latest from the linear chain.
    let getting_specific_block = maybe_id.is_some();
    let maybe_block = effect_builder
        .make_request(
            |responder| RpcRequest::GetBlock {
                maybe_id,
                responder,
            },
            QueueKind::Api,
        )
        .await;

    if maybe_block.is_none() && getting_specific_block {
        info!("failed to get {:?} from storage", maybe_id.unwrap());
        return Err(warp_json_rpc::Error::custom(
            ErrorCode::NoSuchBlock as i64,
            "block not known",
        ));
    }

    Ok(maybe_block)
}
