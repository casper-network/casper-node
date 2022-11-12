use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use derive_more::{Display, From};
use either::Either;
use tracing::{debug, error, info, trace, warn};

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
                write!(f, "invalid attempt to apply execution results checksum")
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

#[derive(Clone, DataSize, Debug)]
pub(super) enum BlockAcquisitionState {
    Initialized(BlockHash, SignatureAcquisition),
    HaveBlockHeader(Box<BlockHeader>, SignatureAcquisition),
    HaveWeakFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    HaveBlock(Box<Block>, SignatureAcquisition, DeployAcquisition),
    HaveGlobalState(
        Box<Block>,
        SignatureAcquisition,
        DeployAcquisition,
        ExecutionResultsAcquisition,
    ),
    HaveAllExecutionResults(
        Box<Block>,
        SignatureAcquisition,
        DeployAcquisition,
        ExecutionResultsChecksum,
    ),
    HaveApprovalsHashes(Box<Block>, SignatureAcquisition, DeployAcquisition),
    HaveAllDeploys(Box<BlockHeader>, SignatureAcquisition),
    HaveStrictFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    Fatal(BlockHash, Option<u64>),
}

impl Display for BlockAcquisitionState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockAcquisitionState::Initialized(block_hash, _) => {
                write!(f, "initialized for: {}", block_hash)
            }
            BlockAcquisitionState::HaveBlockHeader(block_header, _) => write!(
                f,
                "have block header({}) for: {}",
                block_header.height(),
                block_header.block_hash()
            ),
            BlockAcquisitionState::HaveWeakFinalitySignatures(block_header, _) => write!(
                f,
                "have weak finality({}) for: {}",
                block_header.height(),
                block_header.block_hash()
            ),
            BlockAcquisitionState::HaveBlock(block, _, _) => write!(
                f,
                "have block({}) for: {}",
                block.header().height(),
                block.id()
            ),
            BlockAcquisitionState::HaveGlobalState(block, _, _, _) => write!(
                f,
                "have global state({}) for: {}",
                block.header().height(),
                block.id()
            ),
            BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _) => write!(
                f,
                "have execution results({}) for: {}",
                block.header().height(),
                block.id()
            ),
            BlockAcquisitionState::HaveApprovalsHashes(block, _, _) => write!(
                f,
                "have approvals hashes({}) for: {}",
                block.header().height(),
                block.id()
            ),
            BlockAcquisitionState::HaveAllDeploys(block_header, _) => write!(
                f,
                "have deploys({}) for: {}",
                block_header.height(),
                block_header.block_hash()
            ),
            BlockAcquisitionState::HaveStrictFinalitySignatures(block_header, _) => write!(
                f,
                "have strict finality({}) for: {}",
                block_header.height(),
                block_header.block_hash()
            ),
            BlockAcquisitionState::Fatal(block_hash, maybe_block_height) => {
                write!(f, "fatal({:?}) for: {}", maybe_block_height, block_hash)
            }
        }
    }
}

#[derive(Debug, Display)]
#[must_use]
pub(super) enum Acceptance {
    #[display(fmt = "had it")]
    HadIt,
    #[display(fmt = "needed it")]
    NeededIt,
}

impl BlockAcquisitionState {
    pub(super) fn block_height(&self) -> Option<u64> {
        match self {
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Fatal(..) => None,
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

    pub(super) fn register_header(&mut self, header: BlockHeader) -> Result<Acceptance, Error> {
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
            | BlockAcquisitionState::Fatal(..) => return Ok(Acceptance::HadIt),
        };
        self.set_state(new_state);
        Ok(Acceptance::NeededIt)
    }

    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        need_execution_state: bool,
    ) -> Result<Acceptance, Error> {
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
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(Acceptance::HadIt);
            }
        };
        self.set_state(new_state);
        Ok(Acceptance::NeededIt)
    }

    pub(super) fn register_block(
        &mut self,
        block: &Block,
        need_execution_state: bool,
    ) -> Result<Acceptance, Error> {
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
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(Acceptance::HadIt);
            }
        };
        self.set_state(new_state);
        Ok(Acceptance::NeededIt)
    }

    pub(super) fn register_finality_signature(
        &mut self,
        signature: FinalitySignature,
        validator_weights: &EraValidatorWeights,
        need_execution_results: bool,
    ) -> Result<Acceptance, Error> {
        let signer = signature.public_key.clone();
        let mut added = false;
        let mut maybe_block_hash: Option<BlockHash> = None;
        let maybe_new_state: Option<BlockAcquisitionState> = match self {
            BlockAcquisitionState::HaveBlockHeader(header, acquired_signatures) => {
                // we are attempting to acquire at least ~1/3 signature weight before
                // committing to doing non-trivial work to acquire this block
                // thus the primary thing we are doing in this state is accumulating sigs
                maybe_block_hash = Some(header.block_hash());
                added = acquired_signatures.apply_signature(signature);
                match validator_weights.has_sufficient_weight(acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient => None,
                    SignatureWeight::Weak | SignatureWeight::Sufficient => {
                        Some(BlockAcquisitionState::HaveWeakFinalitySignatures(
                            header.clone(),
                            acquired_signatures.clone(),
                        ))
                    }
                }
            }
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, deploy_acquisition) => {
                // In this state we have peers & a block header & at least weak finality sigs weight
                // There are 3 possible flows while in this state:
                // 1: if this is a historical sync we are waiting until GlobalState is acquired
                // 2: else if this block has any deploys, we are waiting to get approvals hashes
                // for those deploys before attempting to acquire the deploys themselves
                // 3: else if this block has no deploys there are no approvals hashes to acquire and
                // we need to acquire strong finality sigs weight, at which point we skip past
                // approvals hashes and deploy acquisition (bcs there aren't any to acquire)
                // to HaveStrictFinalitySignatures
                maybe_block_hash = Some(block.id());
                added = acquired_signatures.apply_signature(signature);
                if need_execution_results || deploy_acquisition.needs_deploy().is_some() {
                    None
                } else {
                    match validator_weights
                        .has_sufficient_weight(acquired_signatures.have_signatures())
                    {
                        SignatureWeight::Insufficient | SignatureWeight::Weak => None,
                        SignatureWeight::Sufficient => {
                            Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                                Box::new(block.header().clone()),
                                acquired_signatures.clone(),
                            ))
                        }
                    }
                }
            }
            BlockAcquisitionState::HaveGlobalState(
                block,
                acquired_signatures,
                deploy_acquisition,
                ..,
            ) if need_execution_results => {
                // In this state, we are in historical mode and need to acquire execution effects
                // 1: if this block has any deploys.
                // 2: else if this block has no deploys there are no execution effects to acquire
                // and we need to acquire strong finality sigs weight, at which point we skip past
                // execution effects, approvals hashes, and deploy acquisition (bcs there aren't
                // any to acquire) to HaveStrictFinalitySignatures
                maybe_block_hash = Some(block.id());
                added = acquired_signatures.apply_signature(signature);
                if deploy_acquisition.needs_deploy().is_some() {
                    None
                } else {
                    match validator_weights
                        .has_sufficient_weight(acquired_signatures.have_signatures())
                    {
                        SignatureWeight::Insufficient | SignatureWeight::Weak => None,
                        SignatureWeight::Sufficient => {
                            Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                                Box::new(block.header().clone()),
                                acquired_signatures.clone(),
                            ))
                        }
                    }
                }
            }

            BlockAcquisitionState::HaveApprovalsHashes(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(block, acquired_signatures, ..) => {
                // in these states this block has at least one deploy and we can't change state
                // until we have acquired all the necessary deploy related data
                maybe_block_hash = Some(block.id());
                added = acquired_signatures.apply_signature(signature);
                None
            }

            BlockAcquisitionState::HaveWeakFinalitySignatures(header, acquired_signatures)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(header, acquired_signatures) => {
                // 1: In HaveWeakFinalitySignatures we are waiting to acquire the block body
                // 2: In HaveStrictFinalitySignatures we are in the happy path resting state
                // and have enough signatures, but not necessarily all signatures and
                // will accept late comers while resting in this state
                maybe_block_hash = Some(header.block_hash());
                added = acquired_signatures.apply_signature(signature);
                None
            }

            BlockAcquisitionState::HaveAllDeploys(header, acquired_signatures) => {
                // We have acquired all the necessary data for this block and are attempting
                // to acquire enough signature weight to get to strict finality; once we do
                // we move to strict finality
                maybe_block_hash = Some(header.block_hash());
                added = acquired_signatures.apply_signature(signature);
                match validator_weights.has_sufficient_weight(acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient | SignatureWeight::Weak => None,
                    SignatureWeight::Sufficient => {
                        Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                            header.clone(),
                            acquired_signatures.clone(),
                        ))
                    }
                }
            }

            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::Fatal(..) => None,
        };
        let ret = if added {
            Acceptance::NeededIt
        } else {
            Acceptance::HadIt
        };
        self.log_finality_signature_acceptance(&maybe_block_hash, &signer, &ret);
        if let Some(new_state) = maybe_new_state {
            self.set_state(new_state);
        }
        Ok(ret)
    }

    pub(super) fn register_marked_complete(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, _)
                if !need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    Box::new(block.header().clone()),
                    acquired_signatures.clone(),
                )
            }
            BlockAcquisitionState::HaveGlobalState(block, acquired_signatures, ..)
                if need_execution_state =>
            {
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

            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
    }

    pub(super) fn register_deploy(
        &mut self,
        deploy_id: DeployId,
        need_execution_state: bool,
    ) -> Result<Acceptance, Error> {
        let (new_state, acceptance) = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, deploys)
                if !need_execution_state =>
            {
                info!("BlockAcquisition: registering deploy for: {}", block.id());
                let acceptance = deploys.apply_deploy(deploy_id);
                match deploys.needs_deploy() {
                    None => {
                        let new_state = BlockAcquisitionState::HaveAllDeploys(
                            Box::new(block.header().clone()),
                            signatures.clone(),
                        );
                        (new_state, acceptance)
                    }
                    Some(_) => {
                        // Should not change state.
                        return Ok(acceptance);
                    }
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(block, signatures, deploys, _)
                if need_execution_state =>
            {
                info!("BlockAcquisition: registering deploy for: {}", block.id());
                let acceptance = deploys.apply_deploy(deploy_id);
                match deploys.needs_deploy() {
                    None => {
                        let new_state = BlockAcquisitionState::HaveAllDeploys(
                            Box::new(block.header().clone()),
                            signatures.clone(),
                        );
                        (new_state, acceptance)
                    }
                    Some(_) => {
                        // Should not change state.
                        return Ok(acceptance);
                    }
                }
            }
            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(Acceptance::HadIt);
            }
        };
        self.set_state(new_state);
        Ok(acceptance)
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
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
    }

    pub(super) fn register_execution_results_checksum(
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
            | BlockAcquisitionState::Fatal(..) => {
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
                        | ExecutionResultsAcquisition::Acquiring { .. } => return Ok(None),
                        ExecutionResultsAcquisition::Acquired { .. } => {
                            let deploy_hashes =
                                block.deploy_and_transfer_hashes().copied().collect();
                            match new_effects.apply_deploy_hashes(deploy_hashes) {
                                Ok(ExecutionResultsAcquisition::Complete {
                                    block_hash,
                                    checksum,
                                    results,
                                }) => (
                                    BlockAcquisitionState::HaveGlobalState(
                                        block.clone(),
                                        signatures.clone(),
                                        deploys.clone(),
                                        ExecutionResultsAcquisition::Complete {
                                            block_hash,
                                            checksum,
                                            results: results.clone(),
                                        },
                                    ),
                                    Some(results),
                                ),
                                Ok(ExecutionResultsAcquisition::Needed { .. })
                                | Ok(ExecutionResultsAcquisition::Pending { .. })
                                | Ok(ExecutionResultsAcquisition::Acquiring { .. })
                                | Ok(ExecutionResultsAcquisition::Acquired { .. }) => {
                                    return Err(Error::InvalidStateTransition)
                                }
                                // todo! - when `apply_deploy_hashes` returns an
                                // `ExecutionResultToDeployHashLengthDiscrepancy`, we must
                                // disconnect from the peer that gave us the execution results
                                // and start over using another peer.
                                Err(error) => return Err(Error::ExecutionResults(error)),
                            }
                        }
                        ExecutionResultsAcquisition::Complete { ref results, .. } => (
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
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(None);
            }
        };
        self.set_state(new_state);
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
                ExecutionResultsAcquisition::Complete { checksum, .. },
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
            | BlockAcquisitionState::Fatal(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
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
                        ExecutionResultsAcquisition::Acquired { .. }
                        | ExecutionResultsAcquisition::Complete { .. } => {
                            Err(Error::InvalidAttemptToAcquireExecutionResults)
                        }
                        ExecutionResultsAcquisition::Needed { .. } => {
                            Ok(BlockAcquisitionAction::execution_results_checksum(
                                *block.hash(),
                                *block.state_root_hash(),
                            ))
                        }
                        acq @ ExecutionResultsAcquisition::Pending { .. }
                        | acq @ ExecutionResultsAcquisition::Acquiring { .. } => {
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
            BlockAcquisitionState::HaveStrictFinalitySignatures(header, ..) => {
                Ok(BlockAcquisitionAction::need_nothing(header.block_hash()))
            }
            BlockAcquisitionState::Fatal(block_hash, ..) => {
                Ok(BlockAcquisitionAction::need_nothing(*block_hash))
            }
        };
        next_action
    }

    fn log_finality_signature_acceptance(
        &self,
        maybe_block_hash: &Option<BlockHash>,
        signer: &PublicKey,
        acceptance: &Acceptance,
    ) {
        match maybe_block_hash {
            None => {
                error!(
                    "BlockAcquisition: unknown block_hash for finality signature from {}",
                    signer
                );
            }
            Some(block_hash) => match acceptance {
                Acceptance::HadIt => {
                    trace!(
                        "BlockAcquisition: existing finality signature for {:?} from {}",
                        block_hash,
                        signer
                    );
                }
                Acceptance::NeededIt => {
                    debug!(
                        "BlockAcquisition: new finality signature for {:?} from {}",
                        block_hash, signer
                    );
                }
            },
        }
    }

    fn set_state(&mut self, new_state: BlockAcquisitionState) {
        debug!(
            "BlockAcquisition: {} (transitioned from: {})",
            new_state, self
        );
        *self = new_state;
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

    pub(super) fn need_nothing(block_hash: BlockHash) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Nothing(block_hash),
        }
    }

    pub(super) fn peers(block_hash: BlockHash) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Peers(block_hash),
        }
    }

    pub(super) fn execution_results_checksum(
        block_hash: BlockHash,
        global_state_root_hash: Digest,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::ExecutionResultsChecksum(block_hash, global_state_root_hash),
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
                need_next: NeedNext::BlockMarkedComplete(block_hash, block_height),
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
