//! RPCs returning ancillary information.

use std::{collections::BTreeMap, str, sync::Arc};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{
    binary_port::MinimalBlockInfo,
    execution::{ExecutionResult, ExecutionResultV2},
    ActivationPoint, AvailableBlockRange, Block, BlockSynchronizerStatus, ChainspecRawBytes,
    Deploy, DeployHash, Digest, EraId, ExecutionInfo, FinalizedApprovals, NextUpgrade, Peers,
    ProtocolVersion, PublicKey, ReactorState, TimeDiff, Timestamp, Transaction, TransactionHash,
    ValidatorChange,
};

use super::{
    common,
    docs::{DocExample, DOCS_EXAMPLE_API_VERSION},
    ApiVersion, Error, NodeClient, RpcError, RpcWithParams, RpcWithoutParams, CURRENT_API_VERSION,
};

static GET_DEPLOY_PARAMS: Lazy<GetDeployParams> = Lazy::new(|| GetDeployParams {
    deploy_hash: *Deploy::doc_example().hash(),
    finalized_approvals: true,
});
static GET_DEPLOY_RESULT: Lazy<GetDeployResult> = Lazy::new(|| GetDeployResult {
    api_version: DOCS_EXAMPLE_API_VERSION,
    deploy: Deploy::doc_example().clone(),
    execution_info: Some(ExecutionInfo {
        block_hash: *Block::example().hash(),
        block_height: Block::example().clone_header().height(),
        execution_result: Some(ExecutionResult::from(ExecutionResultV2::example().clone())),
    }),
});
static GET_TRANSACTION_PARAMS: Lazy<GetTransactionParams> = Lazy::new(|| GetTransactionParams {
    transaction_hash: Transaction::doc_example().hash(),
    finalized_approvals: true,
});
static GET_TRANSACTION_RESULT: Lazy<GetTransactionResult> = Lazy::new(|| GetTransactionResult {
    api_version: DOCS_EXAMPLE_API_VERSION,
    transaction: Transaction::doc_example().clone(),
    execution_info: Some(ExecutionInfo {
        block_hash: *Block::example().hash(),
        block_height: Block::example().height(),
        execution_result: Some(ExecutionResult::from(ExecutionResultV2::example().clone())),
    }),
});
static GET_PEERS_RESULT: Lazy<GetPeersResult> = Lazy::new(|| GetPeersResult {
    api_version: DOCS_EXAMPLE_API_VERSION,
    peers: Some(("tls:0101..0101".to_owned(), "127.0.0.1:54321".to_owned()))
        .into_iter()
        .collect::<BTreeMap<_, _>>()
        .into(),
});
static GET_VALIDATOR_CHANGES_RESULT: Lazy<GetValidatorChangesResult> = Lazy::new(|| {
    let change = JsonValidatorStatusChange::new(EraId::new(1), ValidatorChange::Added);
    let public_key = PublicKey::example().clone();
    let changes = vec![JsonValidatorChanges::new(public_key, vec![change])];
    GetValidatorChangesResult {
        api_version: DOCS_EXAMPLE_API_VERSION,
        changes,
    }
});
static GET_CHAINSPEC_RESULT: Lazy<GetChainspecResult> = Lazy::new(|| GetChainspecResult {
    api_version: DOCS_EXAMPLE_API_VERSION,
    chainspec_bytes: ChainspecRawBytes::new(vec![42, 42].into(), None, None),
});

static GET_STATUS_RESULT: Lazy<GetStatusResult> = Lazy::new(|| GetStatusResult {
    peers: GET_PEERS_RESULT.peers.clone(),
    api_version: DOCS_EXAMPLE_API_VERSION,
    chainspec_name: String::from("casper-example"),
    starting_state_root_hash: Digest::default(),
    last_added_block_info: Some(MinimalBlockInfo::from(Block::example().clone())),
    our_public_signing_key: Some(PublicKey::example().clone()),
    round_length: Some(TimeDiff::from_millis(1 << 16)),
    next_upgrade: Some(NextUpgrade::new(
        ActivationPoint::EraId(EraId::from(42)),
        ProtocolVersion::from_parts(2, 0, 1),
    )),
    uptime: TimeDiff::from_seconds(13),
    reactor_state: ReactorState::Initialize,
    last_progress: Timestamp::from(0),
    available_block_range: AvailableBlockRange::RANGE_0_0,
    block_sync: BlockSynchronizerStatus::example().clone(),
    #[cfg(not(test))]
    build_version: version_string(),

    //  Prevent these values from changing between test sessions
    #[cfg(test)]
    build_version: String::from("1.0.0-xxxxxxxxx@DEBUG"),
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

/// The default for `GetDeployParams::finalized_approvals` and
/// `GetTransactionParams::finalized_approvals`.
fn finalized_approvals_default() -> bool {
    false
}

impl DocExample for GetDeployParams {
    fn doc_example() -> &'static Self {
        &GET_DEPLOY_PARAMS
    }
}

/// Result for "info_get_deploy" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeployResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ApiVersion,
    /// The deploy.
    pub deploy: Deploy,
    /// Execution info, if available.
    #[serde(skip_serializing_if = "Option::is_none", flatten)]
    pub execution_info: Option<ExecutionInfo>,
}

impl DocExample for GetDeployResult {
    fn doc_example() -> &'static Self {
        &GET_DEPLOY_RESULT
    }
}

/// "info_get_deploy" RPC.
pub struct GetDeploy {}

#[async_trait]
impl RpcWithParams for GetDeploy {
    const METHOD: &'static str = "info_get_deploy";
    type RequestParams = GetDeployParams;
    type ResponseResult = GetDeployResult;

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, RpcError> {
        let hash = TransactionHash::from(params.deploy_hash);
        let (transaction, approvals) = common::get_transaction_with_approvals(&*node_client, hash)
            .await
            .map_err(|err| match err {
                Error::NoTransactionWithHash(_) => Error::NoDeployWithHash(params.deploy_hash),
                other => other,
            })?;

        let deploy = match (transaction, approvals) {
            (Transaction::Deploy(deploy), Some(FinalizedApprovals::Deploy(approvals)))
                if params.finalized_approvals =>
            {
                deploy.with_approvals(approvals.into_inner())
            }
            (Transaction::Deploy(deploy), Some(FinalizedApprovals::Deploy(_)) | None) => deploy,
            (Transaction::V1(_), _) => return Err(Error::FoundTransactionInsteadOfDeploy.into()),
            _ => return Err(Error::InconsistentTransactionVersions(hash).into()),
        };

        let execution_info = common::get_transaction_execution_info(&*node_client, hash).await?;

        Ok(Self::ResponseResult {
            api_version: CURRENT_API_VERSION,
            deploy,
            execution_info,
        })
    }
}

/// Params for "info_get_transaction" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTransactionParams {
    /// The transaction hash.
    pub transaction_hash: TransactionHash,
    /// Whether to return the transaction with the finalized approvals substituted. If `false` or
    /// omitted, returns the transaction with the approvals that were originally received by the
    /// node.
    #[serde(default = "finalized_approvals_default")]
    pub finalized_approvals: bool,
}

impl DocExample for GetTransactionParams {
    fn doc_example() -> &'static Self {
        &GET_TRANSACTION_PARAMS
    }
}

/// Result for "info_get_transaction" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTransactionResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ApiVersion,
    /// The transaction.
    pub transaction: Transaction,
    /// Execution info, if available.
    #[serde(skip_serializing_if = "Option::is_none", flatten)]
    pub execution_info: Option<ExecutionInfo>,
}

impl DocExample for GetTransactionResult {
    fn doc_example() -> &'static Self {
        &GET_TRANSACTION_RESULT
    }
}

/// "info_get_transaction" RPC.
pub struct GetTransaction {}

#[async_trait]
impl RpcWithParams for GetTransaction {
    const METHOD: &'static str = "info_get_transaction";
    type RequestParams = GetTransactionParams;
    type ResponseResult = GetTransactionResult;

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, RpcError> {
        let (transaction, approvals) =
            common::get_transaction_with_approvals(&*node_client, params.transaction_hash).await?;

        let transaction = match (transaction, approvals) {
            (Transaction::V1(txn), Some(FinalizedApprovals::V1(approvals)))
                if params.finalized_approvals =>
            {
                Transaction::from(txn.with_approvals(approvals.into_inner()))
            }
            (Transaction::V1(txn), Some(FinalizedApprovals::V1(_)) | None) => {
                Transaction::from(txn)
            }
            (Transaction::Deploy(deploy), Some(FinalizedApprovals::Deploy(approvals)))
                if params.finalized_approvals =>
            {
                Transaction::from(deploy.with_approvals(approvals.into_inner()))
            }
            (Transaction::Deploy(deploy), Some(FinalizedApprovals::Deploy(_)) | None) => {
                Transaction::from(deploy)
            }
            _ => {
                return Err(Error::InconsistentTransactionVersions(params.transaction_hash).into())
            }
        };

        let execution_info =
            common::get_transaction_execution_info(&*node_client, params.transaction_hash).await?;

        Ok(Self::ResponseResult {
            transaction,
            api_version: CURRENT_API_VERSION,
            execution_info,
        })
    }
}

/// Result for "info_get_peers" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetPeersResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ApiVersion,
    /// The node ID and network address of each connected peer.
    pub peers: Peers,
}

impl DocExample for GetPeersResult {
    fn doc_example() -> &'static Self {
        &GET_PEERS_RESULT
    }
}

/// "info_get_peers" RPC.
pub struct GetPeers {}

#[async_trait]
impl RpcWithoutParams for GetPeers {
    const METHOD: &'static str = "info_get_peers";
    type ResponseResult = GetPeersResult;

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let peers = node_client
            .read_peers()
            .await
            .map_err(|err| Error::NodeRequest("peers", err))?;
        Ok(Self::ResponseResult {
            api_version: CURRENT_API_VERSION,
            peers,
        })
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
    pub api_version: ApiVersion,
    /// The validators' status changes.
    pub changes: Vec<JsonValidatorChanges>,
}

impl GetValidatorChangesResult {
    pub(crate) fn new(changes: BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>) -> Self {
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
            api_version: CURRENT_API_VERSION,
            changes,
        }
    }
}

impl DocExample for GetValidatorChangesResult {
    fn doc_example() -> &'static Self {
        &GET_VALIDATOR_CHANGES_RESULT
    }
}

/// "info_get_validator_changes" RPC.
pub struct GetValidatorChanges {}

#[async_trait]
impl RpcWithoutParams for GetValidatorChanges {
    const METHOD: &'static str = "info_get_validator_changes";
    type ResponseResult = GetValidatorChangesResult;

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let changes = node_client
            .read_validator_changes()
            .await
            .map_err(|err| Error::NodeRequest("validator changes", err))?;
        Ok(Self::ResponseResult::new(changes.into()))
    }
}

/// Result for the "info_get_chainspec" RPC.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
pub struct GetChainspecResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ApiVersion,
    /// The chainspec file bytes.
    pub chainspec_bytes: ChainspecRawBytes,
}

impl DocExample for GetChainspecResult {
    fn doc_example() -> &'static Self {
        &GET_CHAINSPEC_RESULT
    }
}

/// "info_get_chainspec" RPC.
pub struct GetChainspec {}

#[async_trait]
impl RpcWithoutParams for GetChainspec {
    const METHOD: &'static str = "info_get_chainspec";
    type ResponseResult = GetChainspecResult;

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let chainspec_bytes = node_client
            .read_chainspec_bytes()
            .await
            .map_err(|err| Error::NodeRequest("chainspec bytes", err))?;

        Ok(Self::ResponseResult {
            api_version: CURRENT_API_VERSION,
            chainspec_bytes,
        })
    }
}

/// Result for "info_get_status" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatusResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ApiVersion,
    /// The node ID and network address of each connected peer.
    pub peers: Peers,
    /// The compiled node version.
    pub build_version: String,
    /// The chainspec name.
    pub chainspec_name: String,
    /// The state root hash of the lowest block in the available block range.
    pub starting_state_root_hash: Digest,
    /// The minimal info of the last block from the linear chain.
    pub last_added_block_info: Option<MinimalBlockInfo>,
    /// Our public signing key.
    pub our_public_signing_key: Option<PublicKey>,
    /// The next round length if this node is a validator.
    pub round_length: Option<TimeDiff>,
    /// Information about the next scheduled upgrade.
    pub next_upgrade: Option<NextUpgrade>,
    /// Time that passed since the node has started.
    pub uptime: TimeDiff,
    /// The current state of node reactor.
    pub reactor_state: ReactorState,
    /// Timestamp of the last recorded progress in the reactor.
    pub last_progress: Timestamp,
    /// The available block range in storage.
    pub available_block_range: AvailableBlockRange,
    /// The status of the block synchronizer builders.
    pub block_sync: BlockSynchronizerStatus,
}

impl DocExample for GetStatusResult {
    fn doc_example() -> &'static Self {
        &GET_STATUS_RESULT
    }
}

/// "info_get_status" RPC.
pub struct GetStatus {}

#[async_trait]
impl RpcWithoutParams for GetStatus {
    const METHOD: &'static str = "info_get_status";
    type ResponseResult = GetStatusResult;

    async fn do_handle_request(
        node_client: Arc<dyn NodeClient>,
    ) -> Result<Self::ResponseResult, RpcError> {
        let status = node_client
            .read_node_status()
            .await
            .map_err(|err| Error::NodeRequest("node status", err))?;

        Ok(Self::ResponseResult {
            api_version: CURRENT_API_VERSION,
            peers: status.peers,
            chainspec_name: status.chainspec_name,
            starting_state_root_hash: status.starting_state_root_hash,
            last_added_block_info: status.last_added_block_info,
            our_public_signing_key: status.our_public_signing_key,
            round_length: status.round_length,
            next_upgrade: status.next_upgrade,
            uptime: status.uptime,
            reactor_state: status.reactor_state,
            last_progress: status.last_progress,
            available_block_range: status.available_block_range,
            block_sync: status.block_sync,
            build_version: status.build_version,
        })
    }
}

#[cfg(not(test))]
fn version_string() -> String {
    use std::env;
    use tracing::warn;

    let mut version = env!("CARGO_PKG_VERSION").to_string();
    if let Ok(git_sha) = env::var("VERGEN_GIT_SHA") {
        version = format!("{}-{}", version, git_sha);
    } else {
        warn!(
            "vergen env var unavailable, casper-node build version will not include git short hash"
        );
    }

    // Add a `@DEBUG` (or similar) tag to release string on non-release builds.
    if env!("SIDECAR_BUILD_PROFILE") != "release" {
        version += "@";
        let profile = env!("SIDECAR_BUILD_PROFILE").to_uppercase();
        version.push_str(&profile);
    }

    version
}
