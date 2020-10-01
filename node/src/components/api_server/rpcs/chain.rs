//! RPCs related to the block chain.

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use super::{
    ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithOptionalParams, RpcWithOptionalParamsExt,
};
use crate::{
    components::api_server::CLIENT_API_VERSION,
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Block, BlockHash},
};

/// Params for "chain_get_block" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockParams {
    /// The block hash.
    pub block_hash: BlockHash,
}

/// Result for "chain_get_block" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The block, if found.
    pub block: Option<Block>,
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
            let maybe_block_hash = maybe_params.map(|params| params.block_hash);
            let maybe_block = match get_block(maybe_block_hash, effect_builder).await {
                Ok(maybe_block) => maybe_block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                block: maybe_block,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Params for "chain_get_global_state_hash" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetGlobalStateHashParams {
    /// The block hash.
    pub block_hash: BlockHash,
}

/// Result for "chain_get_global_state_hash" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetGlobalStateHashResult {
    /// The RPC API version.
    pub api_version: Version,
    /// Hex-encoded global state hash.
    pub global_state_hash: Option<Digest>,
}

/// "chain_get_global_state_hash" RPC.
pub struct GetGlobalStateHash {}

impl RpcWithOptionalParams for GetGlobalStateHash {
    const METHOD: &'static str = "chain_get_global_state_hash";
    type OptionalRequestParams = GetGlobalStateHashParams;
    type ResponseResult = GetGlobalStateHashResult;
}

impl RpcWithOptionalParamsExt for GetGlobalStateHash {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Get the block.
            let maybe_block_hash = maybe_params.map(|params| params.block_hash);
            let maybe_block = match get_block(maybe_block_hash, effect_builder).await {
                Ok(maybe_block) => maybe_block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                global_state_hash: maybe_block.map(|block| *block.global_state_hash()),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

async fn get_block<REv: ReactorEventT>(
    maybe_hash: Option<BlockHash>,
    effect_builder: EffectBuilder<REv>,
) -> Result<Option<Block>, warp_json_rpc::Error> {
    // Get the block from storage or the latest from the linear chain.
    let getting_from_storage = maybe_hash.is_some();
    let maybe_block = effect_builder
        .make_request(
            |responder| ApiRequest::GetBlock {
                maybe_hash,
                responder,
            },
            QueueKind::Api,
        )
        .await;

    if maybe_block.is_none() && getting_from_storage {
        info!("failed to get {} from storage", maybe_hash.unwrap());
        return Err(warp_json_rpc::Error::custom(
            ErrorCode::NoSuchBlock as i64,
            "block not known",
        ));
    }

    Ok(maybe_block)
}
