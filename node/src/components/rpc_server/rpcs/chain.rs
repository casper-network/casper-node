//! RPCs related to the block chain.

use std::str;

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use casper_types::auction::EraId;

use super::{
    Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithOptionalParams, RpcWithOptionalParamsExt,
};
use crate::{
    components::CLIENT_API_VERSION,
    crypto::{
        asymmetric_key::{PublicKey, Signature},
        hash::Digest,
    },
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{
        json_compatibility::KeyValuePair, Block, BlockHash, BlockHeader, DeployHash, Timestamp,
    },
};

/// Identifier for possible ways to retrieve a block.
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum BlockIdentifier {
    /// Identify and retrieve the block with its hash.
    Hash(BlockHash),
    /// Identify and retrieve the block with its height.
    Height(u64),
}

type EraEnd = crate::components::consensus::EraEnd<PublicKey>;

/// Represent the BTreeMap<PublicKey,u64> in an OpenRPC compatible format.
#[derive(Clone, DataSize, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct JsonEraEnd {
    equivocators: Vec<PublicKey>,
    rewards: Vec<KeyValuePair<PublicKey, u64>>,
}

impl JsonEraEnd {
    /// Create a single representation of EraEnd that can be serialized to an OpenRPC compatible
    /// format.
    fn new(equivocators: Vec<PublicKey>, rewards: Vec<KeyValuePair<PublicKey, u64>>) -> Self {
        JsonEraEnd {
            equivocators,
            rewards,
        }
    }
}

impl From<&EraEnd> for JsonEraEnd {
    fn from(era_end: &EraEnd) -> Self {
        let rewards = era_end
            .rewards
            .iter()
            .map(|(public_key, rewards)| KeyValuePair::new(*public_key, *rewards))
            .collect();
        JsonEraEnd::new(era_end.equivocators.clone(), rewards)
    }
}

/// The header portion of a [`Block`](struct.Block.html).
#[derive(Clone, DataSize, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct JsonBlockHeader {
    parent_hash: BlockHash,
    state_root_hash: Digest,
    body_hash: Digest,
    deploy_hashes: Vec<DeployHash>,
    random_bit: bool,
    accumulated_seed: Digest,
    era_end: Option<JsonEraEnd>,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    proposer: PublicKey,
}

impl JsonBlockHeader {
    /// The list of deploy hashes included in the block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        &self.deploy_hashes
    }
}

impl From<BlockHeader> for JsonBlockHeader {
    fn from(block_header: BlockHeader) -> Self {
        JsonBlockHeader {
            parent_hash: *block_header.parent_hash(),
            state_root_hash: *block_header.state_root_hash(),
            body_hash: *block_header.body_hash(),
            deploy_hashes: block_header.deploy_hashes().clone(),
            random_bit: block_header.random_bit(),
            accumulated_seed: block_header.accumulated_seed(),
            era_end: Some(block_header.era_end().unwrap().into()),
            timestamp: block_header.timestamp(),
            era_id: block_header.era_id().0,
            height: block_header.height(),
            proposer: *block_header.proposer(),
        }
    }
}

/// A proto-block after execution, with the resulting post-state-hash.  This is the core component
/// of the Casper linear blockchain represented in an OpenRPC compatible manner.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonBlock {
    hash: BlockHash,
    header: JsonBlockHeader,
    body: (), // TODO: implement body of block
    proofs: Vec<Signature>,
}

impl JsonBlock {
    fn new(hash: BlockHash, header: JsonBlockHeader, proofs: Vec<Signature>) -> Self {
        JsonBlock {
            hash,
            header,
            body: (),
            proofs,
        }
    }

    /// The deploy hashes included in this block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        self.header.deploy_hashes()
    }
}

impl From<Block> for JsonBlock {
    fn from(block: Block) -> Self {
        let header: JsonBlockHeader = block.header().clone().into();
        JsonBlock::new(*block.hash(), header, block.proofs().clone())
    }
}

/// Params for "chain_get_block" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockParams {
    /// The block hash.
    pub block_identifier: BlockIdentifier,
}

/// Result for "chain_get_block" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The block, if found.
    pub block: Option<JsonBlock>,
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
                block: Some(maybe_block.unwrap().into()),
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
    pub block_identifier: BlockIdentifier,
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
