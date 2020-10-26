//! RPCs related to the block chain.

use std::{fmt::{self, Display}, str};

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

/// Enum for Params
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum BlockParameters {
    /// Get a block by its hash. 
    Hash(BlockHash),
    /// Get a block by its height.
    Height(u64),
}


/// Params for "chain_get_block" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockParams {
    /// The block hash.
    pub block_parameter: BlockParameters,
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
            let maybe_block_parameter = maybe_params.map(|params| params.block_parameter);
            let maybe_block = match get_block(maybe_block_parameter, effect_builder).await {
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

/// Params for "chain_get_state_root_hash" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetStateRootHashParams {
    /// The block hash.
    pub block_parameter: BlockParameters,
}

/// Result for "chain_get_state_root_hash" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetStateRootHashResult {
    /// The RPC API version.
    pub api_version: Version,
    /// Hex-encoded hash of the state root.
    pub state_root_hash: Option<Digest>,
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
            let maybe_block_parameter = maybe_params.map(|params| params.block_parameter);
            let maybe_block = match get_block(maybe_block_parameter, effect_builder).await {
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
    maybe_parameter: Option<BlockParameters>,
    effect_builder: EffectBuilder<REv>,
) -> Result<Option<Block>, warp_json_rpc::Error> {
    // Get the block from storage or the latest from the linear chain.
    let getting_from_storage = maybe_parameter.is_some();
    let maybe_block = effect_builder
        .make_request(
            |responder| ApiRequest::GetBlock {
                maybe_parameter,
                responder,
            },
            QueueKind::Api,
        )
        .await;

    if maybe_block.is_none() && getting_from_storage {
        info!("failed to get {} from storage", maybe_parameter.unwrap());
        return Err(warp_json_rpc::Error::custom(
            ErrorCode::NoSuchBlock as i64,
            "block not known",
        ));
    }

    Ok(maybe_block)
}

impl Display for BlockParameters {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BlockParameters::Hash(hash)=> write!(formatter, "{}", hash),
            BlockParameters::Height(height)=> write!(formatter, "{}", height)
        }
    }
}