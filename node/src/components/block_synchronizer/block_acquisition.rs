use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use datasize::DataSize;
use derive_more::From;
use either::Either;
use tracing::{debug, error, info, warn};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use super::deploy_acquisition;

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::DeployAcquisition, need_next::NeedNext, peer_list::PeerList,
        signature_acquisition::SignatureAcquisition, ExecutionResultsAcquisition,
        ExecutionResultsChecksum,
    },
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId,
        BlockHash, BlockHeader, DeployHash, DeployId, EraValidatorWeights, FinalitySignature, Item,
        NodeId, SignatureWeight,
    },
    NodeRng,
};

#[allow(dead_code)] // todo! do a pass on error variants
#[derive(Clone, Copy, From, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    InvalidStateTransition,
    BlockHashMismatch {
        expected: BlockHash,
        actual: BlockHash,
    },
    RootHashMismatch {
        expected: Digest,
        actual: Digest,
    },
    #[from]
    InvalidAttemptToApplyApprovalsHashes(deploy_acquisition::Error),
    InvalidAttemptToApplyGlobalState {
        root_hash: Digest,
    },
    InvalidAttemptToApplyDeploy {
        deploy_id: DeployId,
    },
    InvalidAttemptToApplyExecutionResults,
    InvalidAttemptToApplyExecutionResultsChecksum,
    InvalidAttemptToApplyStoredExecutionResults,
    InvalidAttemptToAcquireExecutionResults,
    InvalidAttemptToMarkComplete,
    ExecutionResults(super::execution_results_acquisition::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidStateTransition => write!(f, "invalid state transition"),
            Error::InvalidAttemptToMarkComplete => {
                write!(f, "invalid attempt to mark complete")
            }
            Error::InvalidAttemptToApplyGlobalState { root_hash } => {
                write!(
                    f,
                    "invalid attempt to apply invalid global hash root hash: {}",
                    root_hash
                )
            }
            Error::InvalidAttemptToApplyDeploy { deploy_id } => {
                write!(f, "invalid attempt to apply invalid deploy {}", deploy_id)
            }
            Error::InvalidAttemptToApplyExecutionResults => {
                write!(f, "invalid attempt to apply execution results")
            }
            Error::InvalidAttemptToApplyExecutionResultsChecksum => {
                write!(f, "invalid attempt to apply execution results root hash")
            }
            Error::InvalidAttemptToApplyStoredExecutionResults => {
                write!(f, "invalid attempt to apply stored execution results notification; execution results are not in terminal state")
            }
            Error::InvalidAttemptToAcquireExecutionResults => {
                write!(
                    f,
                    "invalid attempt to acquire execution results while in a terminal state"
                )
            }
            Error::BlockHashMismatch { expected, actual } => {
                write!(
                    f,
                    "block hash mismatch: expected {} actual: {}",
                    expected, actual
                )
            }
            Error::RootHashMismatch { expected, actual } => write!(
                f,
                "root hash mismatch: expected {} actual: {}",
                expected, actual
            ),
            Error::ExecutionResults(error) => write!(f, "execution results error: {}", error),
            Error::InvalidAttemptToApplyApprovalsHashes(error) => write!(
                f,
                "invalid attempt to apply approvals hashes results: {}",
                error
            ),
        }
    }
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug, derive_more::Display)]
pub(super) enum BlockAcquisitionState {
    #[display(fmt = "Initialize")]
    Initialized(BlockHash, SignatureAcquisition),
    #[display(fmt = "HaveBlockHeader")]
    HaveBlockHeader(Box<BlockHeader>, SignatureAcquisition),
    #[display(fmt = "HaveWeakFinalitySignatures")]
    HaveWeakFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    #[display(fmt = "HaveBlock")]
    HaveBlock(Box<Block>, SignatureAcquisition, DeployAcquisition),
    #[display(fmt = "HaveGlobalState")]
    HaveGlobalState(
        Box<Block>,
        SignatureAcquisition,
        DeployAcquisition,
        ExecutionResultsAcquisition,
    ),
    #[display(fmt = "HaveAllExecutionResults")]
    HaveAllExecutionResults(
        Box<Block>,
        SignatureAcquisition,
        DeployAcquisition,
        ExecutionResultsChecksum,
    ),
    #[display(fmt = "HaveApprovalsHashes")]
    HaveApprovalsHashes(Box<Block>, SignatureAcquisition, DeployAcquisition),
    #[display(fmt = "HaveAllDeploys")]
    HaveAllDeploys(Box<BlockHeader>, SignatureAcquisition),
    #[display(fmt = "HaveStrictFinalitySignatures")]
    HaveStrictFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    #[display(fmt = "Fatal")]
    Fatal,
}

pub(super) enum FinalitySignatureAcceptance {
    Noop,
    NeededIt,
}

impl BlockAcquisitionState {
    pub(super) fn block_height(&self) -> Option<u64> {
        match self {
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Fatal => None,
            BlockAcquisitionState::HaveBlockHeader(header, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(header, _)
            | BlockAcquisitionState::HaveAllDeploys(header, ..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(header, _) => {
                Some(header.height())
            }
            BlockAcquisitionState::HaveBlock(block, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _) => Some(block.height()),
        }
    }

    pub(super) fn register_header(&mut self, header: BlockHeader) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::Initialized(block_hash, signatures) => {
                if header.id() == *block_hash {
                    info!("BlockAcquisition: registering header for: {}", block_hash);
                    BlockAcquisitionState::HaveBlockHeader(Box::new(header), signatures.clone())
                } else {
                    return Err(Error::BlockHashMismatch {
                        expected: *block_hash,
                        actual: header.id(),
                    });
                }
            }
            // we never ask for a block_header while in the following states,
            // and thus it is erroneous to attempt to apply one
            BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => return Ok(()),
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, acquired)
                if !need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering approvals hashes for: {}",
                    block.id()
                );
                acquired.apply_approvals_hashes(approvals_hashes)?;
                BlockAcquisitionState::HaveApprovalsHashes(
                    block.clone(),
                    signatures.clone(),
                    acquired.clone(),
                )
            }

            BlockAcquisitionState::HaveAllExecutionResults(block, signatures, deploys, _)
                if need_execution_state =>
            {
                deploys.apply_approvals_hashes(approvals_hashes)?;
                info!(
                    "BlockAcquisition: registering approvals hashes for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveApprovalsHashes(
                    block.clone(),
                    signatures.clone(),
                    deploys.clone(),
                )
            }
            // we never ask for deploys in the following states, and thus it is erroneous to attempt
            // to apply any
            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn register_block(
        &mut self,
        block: &Block,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, signatures) => {
                let expected_block_hash = header.block_hash();
                let actual_block_hash = block.hash();
                if *actual_block_hash != expected_block_hash {
                    return Err(Error::BlockHashMismatch {
                        expected: expected_block_hash,
                        actual: *actual_block_hash,
                    });
                }
                info!(
                    "BlockAcquisition: registering block for: {}",
                    header.block_hash()
                );
                let deploy_hashes = block.deploy_and_transfer_hashes().copied().collect();
                let deploy_acquisition =
                    DeployAcquisition::new_by_hash(deploy_hashes, need_execution_state);

                BlockAcquisitionState::HaveBlock(
                    Box::new(block.clone()),
                    signatures.clone(),
                    deploy_acquisition,
                )
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn register_finality_signature(
        &mut self,
        signature: FinalitySignature,
        validator_weights: EraValidatorWeights,
    ) -> Result<FinalitySignatureAcceptance, Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlockHeader(header, acquired_signatures)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(header, acquired_signatures)
            | BlockAcquisitionState::HaveAllDeploys(header, acquired_signatures) => {
                if false == acquired_signatures.apply_signature(signature) {
                    return Ok(FinalitySignatureAcceptance::Noop);
                };
                info!(
                    "BlockAcquisition: registering finality signature for: {}",
                    header.block_hash()
                );
                match validator_weights.has_sufficient_weight(acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient => {
                        // Should not change state.
                        return Ok(FinalitySignatureAcceptance::NeededIt);
                    }
                    SignatureWeight::Weak | SignatureWeight::Sufficient => {
                        BlockAcquisitionState::HaveWeakFinalitySignatures(
                            header.clone(),
                            acquired_signatures.clone(),
                        )
                    }
                }
            }
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveGlobalState(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveApprovalsHashes(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(block, acquired_signatures, ..) => {
                if false == acquired_signatures.apply_signature(signature) {
                    return Ok(FinalitySignatureAcceptance::Noop);
                };
                info!(
                    "BlockAcquisition: registering finality signature for: {}",
                    block.id()
                );
                match validator_weights.has_sufficient_weight(acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient | SignatureWeight::Weak => {
                        // Should not change state.
                        return Ok(FinalitySignatureAcceptance::NeededIt);
                    }
                    SignatureWeight::Sufficient => {
                        BlockAcquisitionState::HaveStrictFinalitySignatures(
                            Box::new(block.header().clone()),
                            acquired_signatures.clone(),
                        )
                    }
                }
            }

            // we never ask for finality signatures while in these states, thus it's always
            // erroneous to attempt to apply any
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(FinalitySignatureAcceptance::Noop);
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(FinalitySignatureAcceptance::Noop)
    }

    pub(super) fn register_marked_complete(&mut self) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, _)
            | BlockAcquisitionState::HaveGlobalState(block, acquired_signatures, ..) => {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    Box::new(block.header().clone()),
                    acquired_signatures.clone(),
                )
            }
            BlockAcquisitionState::HaveAllDeploys(header, acquired_signatures) => {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    header.block_hash()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    header.clone(),
                    acquired_signatures.clone(),
                )
            }

            // we never ask for finality signatures while in these states, thus it's always
            // erroneous to attempt to apply any
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn register_deploy(
        &mut self,
        deploy_id: DeployId,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, deploys)
                if !need_execution_state =>
            {
                info!("BlockAcquisition: registering deploy for: {}", block.id());
                deploys.apply_deploy(deploy_id);
                match deploys.needs_deploy() {
                    None => BlockAcquisitionState::HaveAllDeploys(
                        Box::new(block.header().clone()),
                        signatures.clone(),
                    ),
                    Some(_) => {
                        // Should not change state.
                        return Ok(());
                    }
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(block, signatures, deploys, _)
                if need_execution_state =>
            {
                info!("BlockAcquisition: registering deploy for: {}", block.id());
                deploys.apply_deploy(deploy_id);
                match deploys.needs_deploy() {
                    None => BlockAcquisitionState::HaveAllDeploys(
                        Box::new(block.header().clone()),
                        signatures.clone(),
                    ),
                    Some(_) => {
                        // Should not change state.
                        return Ok(());
                    }
                }
            }
            // we never ask for deploys in the following states, and thus it is erroneous to attempt
            // to apply any
            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn register_global_state(
        &mut self,
        root_hash: Digest,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, deploys)
                if need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering global state for: {}",
                    block.id()
                );
                if block.state_root_hash() == &root_hash {
                    let block_hash = *block.hash();
                    BlockAcquisitionState::HaveGlobalState(
                        block.clone(),
                        signatures.clone(),
                        deploys.clone(),
                        ExecutionResultsAcquisition::Needed { block_hash },
                    )
                } else {
                    return Err(Error::RootHashMismatch {
                        expected: *block.state_root_hash(),
                        actual: root_hash,
                    });
                }
            }
            // we never ask for global state in the following states, and thus it is erroneous to
            // attempt to apply any
            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn register_execution_results_root_hash(
        &mut self,
        execution_results_checksum: ExecutionResultsChecksum,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        match self {
            BlockAcquisitionState::HaveGlobalState(
                block,
                _,
                _,
                acq @ ExecutionResultsAcquisition::Needed { .. },
            ) if need_execution_state => {
                info!(
                    "BlockAcquisition: registering execution results hash for: {}",
                    block.id()
                );
                *acq = acq
                    .clone()
                    .apply_checksum(execution_results_checksum)
                    .map_err(Error::ExecutionResults)?;
            }
            BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        Ok(())
    }

    pub(super) fn register_execution_results_or_chunk(
        &mut self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
        need_execution_state: bool,
    ) -> Result<Option<HashMap<DeployHash, casper_types::ExecutionResult>>, Error> {
        let (new_state, ret) = match self {
            BlockAcquisitionState::HaveGlobalState(
                block,
                signatures,
                deploys,
                exec_results_acq,
            ) if need_execution_state => {
                info!(
                    "BlockAcquisition: registering execution result or chunk for: {}",
                    block.id()
                );
                match exec_results_acq
                    .clone()
                    .apply_block_execution_results_or_chunk(block_execution_results_or_chunk)
                {
                    Ok(new_effects) => match new_effects {
                        ExecutionResultsAcquisition::Needed { .. }
                        | ExecutionResultsAcquisition::Pending { .. }
                        | ExecutionResultsAcquisition::Incomplete { .. } => return Ok(None),
                        ExecutionResultsAcquisition::Complete { .. } => {
                            let deploy_hashes =
                                block.deploy_and_transfer_hashes().copied().collect();
                            match new_effects.apply_deploy_hashes(deploy_hashes) {
                                Ok(ExecutionResultsAcquisition::Mapped {
                                    block_hash,
                                    checksum,
                                    results,
                                }) => (
                                    BlockAcquisitionState::HaveGlobalState(
                                        block.clone(),
                                        signatures.clone(),
                                        deploys.clone(),
                                        ExecutionResultsAcquisition::Mapped {
                                            block_hash,
                                            checksum,
                                            results: results.clone(),
                                        },
                                    ),
                                    Some(results),
                                ),
                                Ok(ExecutionResultsAcquisition::Needed { .. })
                                | Ok(ExecutionResultsAcquisition::Pending { .. })
                                | Ok(ExecutionResultsAcquisition::Incomplete { .. })
                                | Ok(ExecutionResultsAcquisition::Complete { .. }) => {
                                    return Err(Error::InvalidStateTransition)
                                }
                                // todo! - when `apply_deploy_hashes` returns an
                                // `ExecutionResultToDeployHashLengthDiscrepancy`, we must
                                // disconnect from the peer that gave us the execution results
                                // and start over using another peer.
                                Err(error) => return Err(Error::ExecutionResults(error)),
                            }
                        }
                        ExecutionResultsAcquisition::Mapped { ref results, .. } => (
                            BlockAcquisitionState::HaveGlobalState(
                                block.clone(),
                                signatures.clone(),
                                deploys.clone(),
                                new_effects.clone(),
                            ),
                            Some(results.clone()),
                        ),
                    },
                    Err(error) => {
                        error!(%error, "failed to apply execution results");
                        return Ok(None);
                    }
                }
            }
            BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(None);
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(ret)
    }

    pub(super) fn register_execution_results_stored_notification(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveGlobalState(
                block,
                signatures,
                deploys,
                ExecutionResultsAcquisition::Mapped { checksum, .. },
            ) if need_execution_state => {
                info!(
                    "BlockAcquisition: registering execution results stored notification for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveAllExecutionResults(
                    block.clone(),
                    signatures.clone(),
                    deploys.clone(),
                    *checksum,
                )
            }
            BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal => {
                return Ok(());
            }
        };
        debug!(
            "BlockAcquisition: current state: {:?} new state: {:?}",
            self, new_state
        );
        *self = new_state;
        Ok(())
    }

    pub(super) fn next_action(
        &mut self,
        peer_list: &PeerList,
        validator_weights: &EraValidatorWeights,
        rng: &mut NodeRng,
        should_fetch_execution_state: bool,
    ) -> Result<BlockAcquisitionAction, Error> {
        let next_action = match self {
            BlockAcquisitionState::Initialized(block_hash, ..) => Ok(
                BlockAcquisitionAction::block_header(peer_list, rng, *block_hash),
            ),
            BlockAcquisitionState::HaveBlockHeader(block_header, signatures) => {
                if validator_weights.is_empty() {
                    Ok(BlockAcquisitionAction::era_validators(
                        validator_weights.era_id(),
                    ))
                } else {
                    Ok(BlockAcquisitionAction::finality_signatures(
                        peer_list,
                        rng,
                        block_header,
                        validator_weights
                            .missing_validators(signatures.have_signatures())
                            .cloned()
                            .collect(),
                    ))
                }
            }
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) => Ok(
                BlockAcquisitionAction::block_body(peer_list, rng, header.id()),
            ),
            BlockAcquisitionState::HaveBlock(block, signatures, deploy_state) => {
                if should_fetch_execution_state {
                    Ok(BlockAcquisitionAction::global_state(
                        peer_list,
                        rng,
                        *block.hash(),
                        *block.state_root_hash(),
                    ))
                } else if deploy_state.needs_deploy().is_none() {
                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                    ))
                } else {
                    Ok(BlockAcquisitionAction::approvals_hashes(
                        block, peer_list, rng,
                    ))
                }
            }
            BlockAcquisitionState::HaveGlobalState(
                block,
                signatures,
                deploy_state,
                exec_results,
            ) => {
                if should_fetch_execution_state == false {
                    Err(Error::InvalidStateTransition)
                } else if deploy_state.needs_deploy().is_none() {
                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                    ))
                } else {
                    match exec_results {
                        ExecutionResultsAcquisition::Complete { .. }
                        | ExecutionResultsAcquisition::Mapped { .. } => {
                            Err(Error::InvalidAttemptToAcquireExecutionResults)
                        }
                        ExecutionResultsAcquisition::Needed { .. } => {
                            Ok(BlockAcquisitionAction::execution_results_root_hash(
                                *block.hash(),
                                *block.state_root_hash(),
                            ))
                        }
                        acq @ ExecutionResultsAcquisition::Pending { .. }
                        | acq @ ExecutionResultsAcquisition::Incomplete { .. } => {
                            match acq.needs_value_or_chunk() {
                                None => {
                                    warn!(block_hash=%block.id(), "execution_results_acquisition.needs_value_or_chunk() should never be None for these variants");
                                    Err(Error::InvalidAttemptToAcquireExecutionResults)
                                }
                                Some((next, checksum)) => {
                                    Ok(BlockAcquisitionAction::execution_results(
                                        block.id(),
                                        peer_list,
                                        rng,
                                        next,
                                        checksum,
                                    ))
                                }
                            }
                        }
                    }
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(
                block,
                signatures,
                deploys,
                checksum,
            ) => {
                if should_fetch_execution_state == false {
                    Err(Error::InvalidStateTransition)
                } else {
                    match deploys.needs_deploy() {
                        Some(missing_deploys) => {
                            if let ExecutionResultsChecksum::Checkable(_) = checksum {
                                Ok(BlockAcquisitionAction::approvals_hashes(
                                    block, peer_list, rng,
                                ))
                            } else {
                                match missing_deploys {
                                    Either::Left(deploy_hash) => {
                                        Ok(BlockAcquisitionAction::deploy_by_hash(
                                            *block.hash(),
                                            deploy_hash,
                                            peer_list,
                                            rng,
                                        ))
                                    }
                                    Either::Right(deploy_id) => {
                                        Ok(BlockAcquisitionAction::deploy_by_id(
                                            *block.hash(),
                                            deploy_id,
                                            peer_list,
                                            rng,
                                        ))
                                    }
                                }
                            }
                        }
                        None => Ok(BlockAcquisitionAction::strict_finality_signatures(
                            peer_list,
                            rng,
                            block.header(),
                            validator_weights,
                            signatures,
                        )),
                    }
                }
            }
            BlockAcquisitionState::HaveApprovalsHashes(block, signatures, deploys) => {
                match deploys.needs_deploy() {
                    Some(Either::Right(deploy_id)) => {
                        info!("BlockAcquisition: requesting missing deploys by ID");
                        Ok(BlockAcquisitionAction::deploy_by_id(
                            *block.hash(),
                            deploy_id,
                            peer_list,
                            rng,
                        ))
                    }
                    Some(Either::Left(deploy_hash)) => {
                        info!("BlockAcquisition: requesting missing deploys by hash");
                        Ok(BlockAcquisitionAction::deploy_by_hash(
                            *block.hash(),
                            deploy_hash,
                            peer_list,
                            rng,
                        ))
                    }
                    None => Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                    )),
                }
            }
            BlockAcquisitionState::HaveAllDeploys(header, signatures) => {
                Ok(BlockAcquisitionAction::strict_finality_signatures(
                    peer_list,
                    rng,
                    header.as_ref(),
                    validator_weights,
                    signatures,
                ))
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(..) => {
                Ok(BlockAcquisitionAction::noop())
            }
            BlockAcquisitionState::Fatal => Ok(BlockAcquisitionAction::noop()),
        };
        debug!("BlockAcquisition: next_action: {:?}", next_action);
        next_action
    }
}

#[derive(Debug)]
pub(crate) struct BlockAcquisitionAction {
    peers_to_ask: Vec<NodeId>,
    need_next: NeedNext,
}

impl BlockAcquisitionAction {
    pub(super) fn need_next(&self) -> NeedNext {
        self.need_next.clone()
    }

    pub(super) fn peers_to_ask(&self) -> Vec<NodeId> {
        self.peers_to_ask.to_vec()
    }

    pub(super) fn noop() -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Nothing,
        }
    }

    pub(super) fn peers(block_hash: BlockHash) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Peers(block_hash),
        }
    }

    pub(super) fn execution_results_root_hash(
        block_hash: BlockHash,
        global_state_root_hash: Digest,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::ExecutionResultsRootHash(block_hash, global_state_root_hash),
        }
    }

    pub(super) fn execution_results(
        block_hash: BlockHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        next: BlockExecutionResultsOrChunkId,
        checksum: ExecutionResultsChecksum,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::ExecutionResults(block_hash, next, checksum),
        }
    }

    pub(super) fn approvals_hashes(block: &Block, peer_list: &PeerList, rng: &mut NodeRng) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::ApprovalsHashes(*block.hash(), Box::new(block.clone())),
        }
    }

    pub(super) fn deploy_by_hash(
        block_hash: BlockHash,
        deploy_hash: DeployHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::DeployByHash(block_hash, deploy_hash),
        }
    }

    pub(super) fn deploy_by_id(
        block_hash: BlockHash,
        deploy_id: DeployId,
        peer_list: &PeerList,
        rng: &mut NodeRng,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::DeployById(block_hash, deploy_id),
        }
    }

    pub(super) fn global_state(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_hash: BlockHash,
        root_hash: Digest,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::GlobalState(block_hash, root_hash),
        }
    }

    pub(super) fn finality_signatures(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_header: &BlockHeader,
        missing_signatures: Vec<PublicKey>,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        let era_id = block_header.era_id();
        let block_hash = block_header.block_hash();
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::FinalitySignatures(block_hash, era_id, missing_signatures),
        }
    }

    pub(super) fn strict_finality_signatures(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_header: &BlockHeader,
        validator_weights: &EraValidatorWeights,
        signature_acquisition: &SignatureAcquisition,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        let era_id = block_header.era_id();
        let block_hash = block_header.block_hash();
        let block_height = block_header.height();

        if let SignatureWeight::Sufficient =
            validator_weights.has_sufficient_weight(signature_acquisition.have_signatures())
        {
            return BlockAcquisitionAction {
                peers_to_ask: vec![],
                need_next: NeedNext::MarkComplete(block_hash, block_height),
            };
        }
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::FinalitySignatures(
                block_hash,
                era_id,
                validator_weights
                    .missing_validators(signature_acquisition.have_signatures())
                    .cloned()
                    .collect(),
            ),
        }
    }

    pub(super) fn block_body(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::BlockBody(block_hash),
        }
    }

    pub(super) fn block_header(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::BlockHeader(block_hash),
        }
    }

    pub(super) fn era_validators(era_id: EraId) -> Self {
        let peers_to_ask = Default::default();
        let need_next = NeedNext::EraValidators(era_id);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next,
        }
    }
}
