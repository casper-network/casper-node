use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::Serialize;
use tracing::info;
use warp_json_rpc::Builder;

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithOptionalParams};
use crate::{
    components::api_server::CLIENT_API_VERSION,
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Block, BlockHash},
};

pub(in crate::components::api_server) struct GetBlock {}

impl RpcWithOptionalParams for GetBlock {
    const METHOD: &'static str = "chain_get_block";

    type RequestParams = String; // Block hash.

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Option<Self::RequestParams>,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            /// JSON-encoded block.
            block: Option<String>,
        }

        async move {
            // Get the block.
            let maybe_block = match get_block(params, effect_builder).await {
                Ok(maybe_block) => maybe_block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let maybe_block_as_json = match maybe_block {
                Some(block) => match block.to_json() {
                    Ok(block_as_json) => Some(block_as_json),
                    Err(error) => {
                        info!("failed to encode block to JSON: {}", error);
                        return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                    }
                },
                None => None,
            };

            let result = ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                block: maybe_block_as_json,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

pub(in crate::components::api_server) struct GetGlobalStateHash {}

impl RpcWithOptionalParams for GetGlobalStateHash {
    const METHOD: &'static str = "chain_get_global_state_hash";

    type RequestParams = String; // Block hash.

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Option<Self::RequestParams>,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            /// hex-encoded global state hash.
            global_state_hash: Option<String>,
        }

        async move {
            // Get the block.
            let maybe_block = match get_block(params, effect_builder).await {
                Ok(maybe_block) => maybe_block,
                Err(error) => return Ok(response_builder.error(error)?),
            };

            // Return the result.
            let result = ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                global_state_hash: maybe_block.map(|block| hex::encode(block.global_state_hash())),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

async fn get_block<REv: ReactorEventT>(
    maybe_hex_block_hash: Option<String>,
    effect_builder: EffectBuilder<REv>,
) -> Result<Option<Block>, warp_json_rpc::Error> {
    // Get the block from storage or the latest from the linear chain.
    if let Some(hex_block_hash) = maybe_hex_block_hash {
        // Try to parse the block hash.
        let block_hash = match Digest::from_hex(&hex_block_hash).map_err(|error| error.to_string())
        {
            Ok(digest) => BlockHash::new(digest),
            Err(error_msg) => {
                info!("failed to parse block hash: {}", error_msg);
                return Err(warp_json_rpc::Error::custom(
                    ErrorCode::ParseBlockHash as i64,
                    error_msg,
                ));
            }
        };

        // Try to get the block from storage.
        let maybe_block = effect_builder
            .make_request(
                |responder| ApiRequest::GetBlock {
                    maybe_hash: Some(block_hash),
                    responder,
                },
                QueueKind::Api,
            )
            .await;

        if maybe_block.is_none() {
            info!("failed to get {} from storage", block_hash);
            return Err(warp_json_rpc::Error::custom(
                ErrorCode::NoSuchBlock as i64,
                "block not known",
            ));
        }

        Ok(maybe_block)
    } else {
        // Get the latest block from the linear chain.
        Ok(effect_builder
            .make_request(
                |responder| ApiRequest::GetBlock {
                    maybe_hash: None,
                    responder,
                },
                QueueKind::Api,
            )
            .await)
    }
}
