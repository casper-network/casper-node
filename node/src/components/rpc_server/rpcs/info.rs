//! RPCs returning ancillary information.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{collections::BTreeMap, str};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use casper_types::{EraId, ExecutionResult, ProtocolVersion, PublicKey};

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithParams, RpcWithoutParams,
};
use crate::{
    components::consensus::ValidatorChange,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{
        Block, BlockHash, BlockHashAndHeight, ChainspecRawBytes, Deploy, DeployHash,
        DeployMetadataExt, GetStatusResult, PeersMap,
    },
};

static GET_DEPLOY_PARAMS: Lazy<GetDeployParams> = Lazy::new(|| GetDeployParams {
    deploy_hash: *Deploy::doc_example().id(),
    finalized_approvals: true,
});
static GET_DEPLOY_RESULT: Lazy<GetDeployResult> = Lazy::new(|| GetDeployResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    deploy: Deploy::doc_example().clone(),
    execution_results: vec![JsonExecutionResult {
        block_hash: *Block::doc_example().hash(),
        result: ExecutionResult::example().clone(),
    }],
    block_hash_and_height: None,
});
static GET_PEERS_RESULT: Lazy<GetPeersResult> = Lazy::new(|| GetPeersResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    peers: GetStatusResult::doc_example().peers.clone(),
});
static GET_VALIDATOR_CHANGES_RESULT: Lazy<GetValidatorChangesResult> = Lazy::new(|| {
    let change = JsonValidatorStatusChange::new(EraId::new(1), ValidatorChange::Added);
    let public_key = PublicKey::doc_example().clone();
    let changes = vec![JsonValidatorChanges::new(public_key, vec![change])];
    GetValidatorChangesResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        changes,
    }
});
static GET_CHAINSPEC_RESULT: Lazy<GetChainspecResult> = Lazy::new(|| GetChainspecResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    chainspec_bytes: ChainspecRawBytes::new(vec![42, 42].into(), None, None),
});

/// Params for "info_get_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeployParams {
    /// The deploy hash.
    pub deploy_hash: DeployHash,
    /// Whether to return the deploy with the finalized approvals substituted. If `false` or
    /// omitted, returns the deploy with the approvals that were originally received by the node.
    #[serde(default = "finalized_approvals_default")]
    pub finalized_approvals: bool,
}

/// The default for `GetDeployParams::finalized_approvals`.
fn finalized_approvals_default() -> bool {
    false
}

impl DocExample for GetDeployParams {
    fn doc_example() -> &'static Self {
        &*GET_DEPLOY_PARAMS
    }
}

/// The execution result of a single deploy.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonExecutionResult {
    /// The block hash.
    pub block_hash: BlockHash,
    /// Execution result.
    pub result: ExecutionResult,
}

/// Result for "info_get_deploy" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeployResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The deploy.
    pub deploy: Deploy,
    /// The map of block hash to execution result.
    pub execution_results: Vec<JsonExecutionResult>,
    /// The hash and height of the block in which this deploy was executed,
    /// only provided if the full execution results are not know on this node.
    #[serde(skip_serializing_if = "Option::is_none", flatten)]
    pub block_hash_and_height: Option<BlockHashAndHeight>,
}

impl DocExample for GetDeployResult {
    fn doc_example() -> &'static Self {
        &*GET_DEPLOY_RESULT
    }
}

/// "info_get_deploy" RPC.
pub struct GetDeploy {}

#[async_trait]
impl RpcWithParams for GetDeploy {
    const METHOD: &'static str = "info_get_deploy";
    type RequestParams = GetDeployParams;
    type ResponseResult = GetDeployResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        // Try to get the deploy and metadata from storage.
        let maybe_deploy_and_metadata = effect_builder
            .make_request(
                |responder| RpcRequest::GetDeploy {
                    hash: params.deploy_hash,
                    finalized_approvals: params.finalized_approvals,
                    responder,
                },
                QueueKind::Api,
            )
            .await;

        let (deploy, metadata_ext) = match maybe_deploy_and_metadata {
            Some((deploy, metadata_ext)) => (deploy, metadata_ext),
            None => {
                let message = format!(
                    "failed to get {} and metadata from storage",
                    params.deploy_hash
                );
                info!("{}", message);
                return Err(Error::new(ErrorCode::NoSuchDeploy, message));
            }
        };

        let (execution_results, block_hash_and_height) = match metadata_ext {
            DeployMetadataExt::Metadata(metadata) => (
                metadata
                    .execution_results
                    .into_iter()
                    .map(|(block_hash, result)| JsonExecutionResult { block_hash, result })
                    .collect(),
                None,
            ),
            DeployMetadataExt::BlockInfo(block_hash_and_height) => {
                (Vec::new(), Some(block_hash_and_height))
            }
            DeployMetadataExt::Empty => (Vec::new(), None),
        };

        let result = Self::ResponseResult {
            api_version,
            deploy,
            execution_results,
            block_hash_and_height,
        };
        Ok(result)
    }
}

/// Result for "info_get_peers" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetPeersResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The node ID and network address of each connected peer.
    pub peers: PeersMap,
}

impl DocExample for GetPeersResult {
    fn doc_example() -> &'static Self {
        &*GET_PEERS_RESULT
    }
}

/// "info_get_peers" RPC.
pub struct GetPeers {}

#[async_trait]
impl RpcWithoutParams for GetPeers {
    const METHOD: &'static str = "info_get_peers";
    type ResponseResult = GetPeersResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> Result<Self::ResponseResult, Error> {
        let peers = effect_builder
            .make_request(
                |responder| RpcRequest::GetPeers { responder },
                QueueKind::Api,
            )
            .await;

        let result = Self::ResponseResult {
            api_version,
            peers: PeersMap::from(peers),
        };
        Ok(result)
    }
}

/// "info_get_status" RPC.
pub struct GetStatus {}

#[async_trait]
impl RpcWithoutParams for GetStatus {
    const METHOD: &'static str = "info_get_status";
    type ResponseResult = GetStatusResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> Result<Self::ResponseResult, Error> {
        // Get the status.
        let status_feed = effect_builder
            .make_request(
                |responder| RpcRequest::GetStatus { responder },
                QueueKind::Api,
            )
            .await;

        // Convert to `ResponseResult` and send.
        let result = Self::ResponseResult::new(status_feed, api_version);
        Ok(result)
    }
}

/// A single change to a validator's status in the given era.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonValidatorStatusChange {
    /// The era in which the change occurred.
    era_id: EraId,
    /// The change in validator status.
    validator_change: ValidatorChange,
}

impl JsonValidatorStatusChange {
    pub(crate) fn new(era_id: EraId, validator_change: ValidatorChange) -> Self {
        JsonValidatorStatusChange {
            era_id,
            validator_change,
        }
    }
}

/// The changes in a validator's status.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonValidatorChanges {
    /// The public key of the validator.
    public_key: PublicKey,
    /// The set of changes to the validator's status.
    status_changes: Vec<JsonValidatorStatusChange>,
}

impl JsonValidatorChanges {
    pub(crate) fn new(
        public_key: PublicKey,
        status_changes: Vec<JsonValidatorStatusChange>,
    ) -> Self {
        JsonValidatorChanges {
            public_key,
            status_changes,
        }
    }
}

/// Result for the "info_get_validator_changes" RPC.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetValidatorChangesResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The validators' status changes.
    pub changes: Vec<JsonValidatorChanges>,
}

impl GetValidatorChangesResult {
    pub(crate) fn new(
        api_version: ProtocolVersion,
        changes: BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>,
    ) -> Self {
        let changes = changes
            .into_iter()
            .map(|(public_key, mut validator_changes)| {
                validator_changes.sort();
                let status_changes = validator_changes
                    .into_iter()
                    .map(|(era_id, validator_change)| {
                        JsonValidatorStatusChange::new(era_id, validator_change)
                    })
                    .collect();
                JsonValidatorChanges::new(public_key, status_changes)
            })
            .collect();
        GetValidatorChangesResult {
            api_version,
            changes,
        }
    }
}

impl DocExample for GetValidatorChangesResult {
    fn doc_example() -> &'static Self {
        &*GET_VALIDATOR_CHANGES_RESULT
    }
}

/// "info_get_validator_changes" RPC.
pub struct GetValidatorChanges {}

#[async_trait]
impl RpcWithoutParams for GetValidatorChanges {
    const METHOD: &'static str = "info_get_validator_changes";
    type ResponseResult = GetValidatorChangesResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> Result<Self::ResponseResult, Error> {
        let changes = effect_builder.get_consensus_validator_changes().await;
        let result = Self::ResponseResult::new(api_version, changes);
        Ok(result)
    }
}

/// Result for the "info_get_chainspec" RPC.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
pub struct GetChainspecResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The chainspec file bytes.
    pub chainspec_bytes: ChainspecRawBytes,
}

impl GetChainspecResult {
    pub(crate) fn new(api_version: ProtocolVersion, chainspec_bytes: ChainspecRawBytes) -> Self {
        Self {
            api_version,
            chainspec_bytes,
        }
    }
}

impl DocExample for GetChainspecResult {
    fn doc_example() -> &'static Self {
        &*GET_CHAINSPEC_RESULT
    }
}

/// "info_get_chainspec" RPC.
pub struct GetChainspec {}

#[async_trait]
impl RpcWithoutParams for GetChainspec {
    const METHOD: &'static str = "info_get_chainspec";
    type ResponseResult = GetChainspecResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> Result<Self::ResponseResult, Error> {
        let chainspec_bytes = effect_builder.get_chainspec_raw_bytes().await;
        let result = Self::ResponseResult::new(api_version, (*chainspec_bytes).clone());
        Ok(result)
    }
}
