use std::{collections::BTreeMap, sync::Arc};

use casper_execution_engine::engine_state::WasmV1Result;
use datasize::DataSize;
use serde::Serialize;
use tracing::{debug, trace};

use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{BalanceHoldResult, BiddingResult, EraValidatorsRequest, TransferResult},
};
use casper_types::{
    contract_messages::Messages,
    execution::{Effects, ExecutionResult, ExecutionResultV2},
    BlockHash, BlockHeaderV2, BlockV2, DeployHash, DeployHeader, Digest, EraId, InvalidDeploy,
    InvalidTransaction, InvalidTransactionV1, ProtocolVersion, PublicKey, Transaction,
    TransactionHash, TransactionHeader, TransactionV1Hash, TransactionV1Header, U512,
};

/// Request for validator weights for a specific era.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorWeightsByEraIdRequest {
    state_hash: Digest,
    era_id: EraId,
    protocol_version: ProtocolVersion,
}

impl ValidatorWeightsByEraIdRequest {
    /// Constructs a new ValidatorWeightsByEraIdRequest.
    pub fn new(state_hash: Digest, era_id: EraId, protocol_version: ProtocolVersion) -> Self {
        ValidatorWeightsByEraIdRequest {
            state_hash,
            era_id,
            protocol_version,
        }
    }

    /// Get the state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Get the era id.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Get the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

impl From<ValidatorWeightsByEraIdRequest> for EraValidatorsRequest {
    fn from(input: ValidatorWeightsByEraIdRequest) -> Self {
        EraValidatorsRequest::new(input.state_hash, input.protocol_version)
    }
}

/// Effects from running step and the next era validators that are gathered when an era ends.
#[derive(Clone, Debug, DataSize)]
pub(crate) struct StepOutcome {
    /// Validator sets for all upcoming eras that have already been determined.
    pub(crate) upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    /// An [`Effects`] created by an era ending.
    pub(crate) step_effects: Effects,
}

pub(crate) struct ExecutionArtifacts {
    artifacts: Vec<ExecutionArtifact>,
}

pub(crate) enum ExecutionArtifactOutcome {
    RootNotFound,
    Effects(Effects),
}

impl ExecutionArtifacts {
    pub fn with_capacity(capacity: usize) -> Self {
        let artifacts = Vec::with_capacity(capacity);
        ExecutionArtifacts { artifacts }
    }

    pub fn push_wasm_v1_result(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        wasm_v1_result: WasmV1Result,
    ) -> ExecutionArtifactOutcome {
        trace!(?transaction_hash, ?wasm_v1_result, "native transfer result");
        let effects = wasm_v1_result.effects().clone();
        let result = {
            if let Some(error) = wasm_v1_result.error() {
                if let casper_execution_engine::engine_state::Error::RootNotFound(_) = error {
                    return ExecutionArtifactOutcome::RootNotFound;
                }
                ExecutionResultV2::Failure {
                    effects: effects.clone(),
                    transfers: wasm_v1_result.transfers().clone(),
                    cost: wasm_v1_result.consumed().value(),
                    error_message: format!("{:?}", error),
                }
            } else {
                ExecutionResultV2::Success {
                    effects: effects.clone(),
                    transfers: wasm_v1_result.transfers().clone(),
                    cost: wasm_v1_result.consumed().value(),
                }
            }
        };
        self.artifacts.push(ExecutionArtifact::new(
            transaction_hash,
            transaction_header,
            ExecutionResult::V2(result),
            wasm_v1_result.messages().clone(),
        ));

        ExecutionArtifactOutcome::Effects(effects)
    }

    pub fn push_transfer_result(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        transfer_result: TransferResult,
        cost: U512,
    ) -> ExecutionArtifactOutcome {
        trace!(
            ?transaction_hash,
            ?transfer_result,
            "native transfer result"
        );
        match transfer_result {
            TransferResult::RootNotFound => ExecutionArtifactOutcome::RootNotFound,
            TransferResult::Failure(transfer_error) => {
                self.push_failure(
                    transaction_hash,
                    transaction_header,
                    format!("{:?}", transfer_error),
                );
                debug!(%transfer_error);
                ExecutionArtifactOutcome::Effects(Effects::new())
            }
            TransferResult::Success {
                effects: transfer_effects,
                transfers,
            } => {
                self.artifacts.push(ExecutionArtifact::new(
                    transaction_hash,
                    transaction_header,
                    ExecutionResult::V2(ExecutionResultV2::Success {
                        effects: transfer_effects.clone(),
                        cost,
                        transfers,
                    }),
                    Messages::default(),
                ));
                ExecutionArtifactOutcome::Effects(transfer_effects)
            }
        }
    }

    pub fn push_bidding_result(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        bidding_result: BiddingResult,
        cost: U512,
    ) -> ExecutionArtifactOutcome {
        trace!(?transaction_hash, ?bidding_result, "bidding result");
        match bidding_result {
            BiddingResult::RootNotFound => ExecutionArtifactOutcome::RootNotFound,
            BiddingResult::Failure(tce) => {
                self.artifacts.push(ExecutionArtifact::new(
                    transaction_hash,
                    transaction_header,
                    ExecutionResult::V2(ExecutionResultV2::Failure {
                        effects: Effects::new(),
                        cost,
                        transfers: vec![],
                        error_message: format!("{:?}", tce),
                    }),
                    Messages::default(),
                ));
                debug!(%tce);
                ExecutionArtifactOutcome::Effects(Effects::new())
            }
            BiddingResult::Success {
                effects: bidding_effects,
                ..
            } => {
                self.artifacts.push(ExecutionArtifact::new(
                    transaction_hash,
                    transaction_header,
                    ExecutionResult::V2(ExecutionResultV2::Success {
                        effects: bidding_effects.clone(),
                        cost,
                        transfers: vec![],
                    }),
                    Messages::default(),
                ));
                ExecutionArtifactOutcome::Effects(bidding_effects)
            }
        }
    }

    pub fn push_hold_result(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        hold_result: BalanceHoldResult,
    ) -> ExecutionArtifactOutcome {
        trace!(?transaction_hash, ?hold_result, "balance hold result");
        if hold_result.is_root_not_found() {
            return ExecutionArtifactOutcome::RootNotFound;
        }
        let hold_cost = U512::zero(); // we don't charge for the hold itself.
        let hold_effects = hold_result.effects();
        if !hold_result.is_fully_covered() {
            let error_message = hold_result.error_message();
            self.artifacts.push(ExecutionArtifact::new(
                transaction_hash,
                transaction_header,
                ExecutionResult::V2(ExecutionResultV2::Failure {
                    effects: hold_effects.clone(),
                    transfers: vec![],
                    cost: hold_cost,
                    error_message: error_message.clone(),
                }),
                Messages::default(),
            ));
            debug!(%error_message);
        } else {
            self.artifacts.push(ExecutionArtifact::new(
                transaction_hash,
                transaction_header,
                ExecutionResult::V2(ExecutionResultV2::Success {
                    effects: hold_effects.clone(),
                    transfers: vec![],
                    cost: hold_cost,
                }),
                Messages::default(),
            ));
        }
        ExecutionArtifactOutcome::Effects(hold_effects)
    }

    pub fn push_invalid_transaction(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        invalid_transaction: InvalidTransaction,
    ) {
        self.push_failure(
            transaction_hash,
            transaction_header,
            format!("{:?}", invalid_transaction),
        );
    }

    pub fn push_auction_method_failure(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        cost: U512,
    ) {
        let msg = "failed to resolve auction method".to_string();
        let artifact = ExecutionArtifact::new(
            transaction_hash,
            transaction_header,
            ExecutionResult::V2(ExecutionResultV2::Failure {
                effects: Effects::new(),
                transfers: vec![],
                error_message: msg.clone(),
                cost,
            }),
            Messages::default(),
        );
        debug!(%transaction_hash, "{:?}", msg);
        self.artifacts.push(artifact);
    }

    fn push_failure(
        &mut self,
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        error_message: String,
    ) {
        let execution_artifact = ExecutionArtifact::new(
            transaction_hash,
            transaction_header,
            ExecutionResult::V2(ExecutionResultV2::Failure {
                effects: Effects::new(),
                cost: U512::zero(),
                transfers: vec![],
                error_message,
            }),
            Messages::default(),
        );
        self.artifacts.push(execution_artifact);
    }

    pub fn execution_results(&self) -> Vec<&ExecutionResult> {
        self.artifacts
            .iter()
            .map(|artifact| &artifact.execution_result)
            .collect::<Vec<&ExecutionResult>>()
    }

    pub fn take(self) -> Vec<ExecutionArtifact> {
        self.artifacts
    }
}

#[derive(Clone, Debug, DataSize, PartialEq, Eq, Serialize)]
pub(crate) struct ExecutionArtifact {
    pub(crate) transaction_hash: TransactionHash,
    pub(crate) header: TransactionHeader,
    pub(crate) execution_result: ExecutionResult,
    pub(crate) messages: Messages,
}

impl ExecutionArtifact {
    pub(crate) fn new(
        transaction_hash: TransactionHash,
        header: TransactionHeader,
        execution_result: ExecutionResult,
        messages: Messages,
    ) -> Self {
        Self {
            transaction_hash,
            header,
            execution_result,
            messages,
        }
    }

    #[allow(unused)]
    pub(crate) fn deploy(
        deploy_hash: DeployHash,
        header: DeployHeader,
        execution_result: ExecutionResult,
        messages: Messages,
    ) -> Self {
        Self {
            transaction_hash: TransactionHash::Deploy(deploy_hash),
            header: TransactionHeader::Deploy(header),
            execution_result,
            messages,
        }
    }

    #[allow(unused)]
    pub(crate) fn v1(
        transaction_hash: TransactionV1Hash,
        header: TransactionV1Header,
        execution_result: ExecutionResult,
        messages: Messages,
    ) -> Self {
        Self {
            transaction_hash: TransactionHash::V1(transaction_hash),
            header: TransactionHeader::V1(header),
            execution_result,
            messages,
        }
    }
}

#[doc(hidden)]
/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Clone, Debug, DataSize)]
pub struct BlockAndExecutionArtifacts {
    /// The [`Block`] the contract runtime executed.
    pub(crate) block: Arc<BlockV2>,
    /// The [`ApprovalsHashes`] for the deploys in this block.
    pub(crate) approvals_hashes: Box<ApprovalsHashes>,
    /// The results from executing the transactions in the block.
    pub(crate) execution_artifacts: Vec<ExecutionArtifact>,
    /// The [`Effects`] and the upcoming validator sets determined by the `step`
    pub(crate) step_outcome: Option<StepOutcome>,
}

/// Type representing results of the speculative execution.
#[derive(Debug)]
pub enum SpeculativeExecutionResult {
    InvalidTransaction(InvalidTransaction),
    WasmV1(WasmV1Result),
}

impl SpeculativeExecutionResult {
    pub fn invalid_gas_limit(transaction: Transaction) -> Self {
        match transaction {
            Transaction::Deploy(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::Deploy(InvalidDeploy::UnableToCalculateGasLimit),
            ),
            Transaction::V1(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::V1(InvalidTransactionV1::UnableToCalculateGasLimit),
            ),
        }
    }
}

/// State to use to construct the next block in the blockchain. Includes the state root hash for the
/// execution engine as well as certain values the next header will be based on.
#[derive(DataSize, Default, Debug, Clone, Serialize)]
pub struct ExecutionPreState {
    /// The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    next_block_height: u64,
    /// The state root to use when executing deploys.
    pre_state_root_hash: Digest,
    /// The parent hash of the next `Block`.
    parent_hash: BlockHash,
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    parent_seed: Digest,
}

impl ExecutionPreState {
    pub(crate) fn new(
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) -> Self {
        ExecutionPreState {
            next_block_height,
            pre_state_root_hash,
            parent_hash,
            parent_seed,
        }
    }

    /// Creates instance of `ExecutionPreState` from given block header nad Merkle tree hash
    /// activation point.
    pub fn from_block_header(block_header: &BlockHeaderV2) -> Self {
        ExecutionPreState {
            pre_state_root_hash: *block_header.state_root_hash(),
            next_block_height: block_header.height() + 1,
            parent_hash: block_header.block_hash(),
            parent_seed: *block_header.accumulated_seed(),
        }
    }

    // The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    pub fn next_block_height(&self) -> u64 {
        self.next_block_height
    }
    /// The state root to use when executing deploys.
    pub fn pre_state_root_hash(&self) -> Digest {
        self.pre_state_root_hash
    }
    /// The parent hash of the next `Block`.
    pub fn parent_hash(&self) -> BlockHash {
        self.parent_hash
    }
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    pub fn parent_seed(&self) -> Digest {
        self.parent_seed
    }
}
