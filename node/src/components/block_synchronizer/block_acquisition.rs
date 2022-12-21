use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use derive_more::Display;
use tracing::{debug, error, info, trace, warn};

use casper_hashing::Digest;
use casper_types::PublicKey;

use crate::{
    components::block_synchronizer::{
        block_acquisition_action::BlockAcquisitionAction, deploy_acquisition::DeployAcquisition,
        peer_list::PeerList, signature_acquisition::SignatureAcquisition, BlockAcquisitionError,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash, BlockHeader, DeployHash,
        DeployId, EraValidatorWeights, FinalitySignature, Item, SignatureWeight,
    },
    NodeRng,
};

// BlockAcquisitionState is a milestone oriented state machine; it is always in a resting state
// indicating the last completed step, while attempting to acquire the necessary data to transition
// to the next resting state milestone. the start and end of the workflow is linear, but the
// middle steps conditionally branch depending upon if this is a historical block (needs execution
// state) or a block we intend to execute, and if the block body has one or more deploys.
//
// blocks always require a header & body and sufficient finality signatures; blocks may contain
// one or more deploys. if a block has any deploys, we must also acquire execution effects
// for the deploys in the block (we do this as a chunked aggregate), and for post 1.5 blocks
// we must also acquire approvals hashes (which correlate to which authorized account holders
// signed the deploys).
//
// there are two levels of finality, weak and strict. we first get the block header (which is
// the minimum amount of necessary information we need to function), and then attempt to acquire
// at least weak finality before doing further work acquiring data for a block, to avoid being
// tricked into wasting resources downloading bogus blocks. with at least weak finality, we can
// go about acquiring the rest of the block's required records relatively safely. if we have not
// acquired strong finality by the time we've downloaded everything else, we do another round
// of asking for remaining signatures before accepting the sync'd block.
//
// when acquiring data for a historical block, we want global state (always) and execution
// effects (if any). when acquiring sufficient data to execute a block, we do not acquire
// global state or execution effects. however, we still check for existence of an execution
// effect _checksum_ leaf in global state at the block's root hash as an expedient way to
// determine if a block was created post-1.5
//
// note that fetchers are used to acquire the required records, which by default check local
// storage for existence and only ask peers if we don't already have the record being fetched
// similarly, we collect finality signatures during each state between HaveBlockHeader and
// HaveStrictFinalitySignatures inclusive, and therefore may have already acquired strict
// finality before we check for it at the very end. finally due to the trie store structure
// of global state, other than the first downloaded historical block we likely already have
// the vast majority of global state data locally. for these reasons, it is common for most
// blocks to transition thru the various states very quickly...particularly blocks without
// deploys. however, the first block downloaded or blocks with a lot of deploys and / or
// execution state delta can take arbitrarily longer on their relevant steps.
//
// similarly, it is possible that the peer set available to us to acquire this data can become
// partitioned. the block synchronizer will periodically attempt to refresh its peer list to
// mitigate this, but this strategy is less effective on small networks. we periodically
// reattempt until we succeed or the node shuts down, in which case: ¯\_(ツ)_/¯
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
    HaveAllDeploys(Box<Block>, SignatureAcquisition),
    HaveStrictFinalitySignatures(Box<Block>, SignatureAcquisition),
    Failed(BlockHash, Option<u64>),
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
                "have block body({}) for: {}",
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
            BlockAcquisitionState::HaveAllDeploys(block, _) => write!(
                f,
                "have deploys({}) for: {}",
                block.header().height(),
                block.id()
            ),
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => write!(
                f,
                "have strict finality({}) for: {}",
                block.header().height(),
                block.id()
            ),
            BlockAcquisitionState::Failed(block_hash, maybe_block_height) => {
                write!(f, "fatal({:?}) for: {}", maybe_block_height, block_hash)
            }
        }
    }
}

impl BlockAcquisitionState {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            BlockAcquisitionState::Initialized(block_hash, _)
            | BlockAcquisitionState::Failed(block_hash, _) => *block_hash,
            BlockAcquisitionState::HaveBlockHeader(block_header, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(block_header, _) => {
                block_header.block_hash()
            }
            BlockAcquisitionState::HaveBlock(block, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _)
            | BlockAcquisitionState::HaveAllDeploys(block, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => block.id(),
        }
    }

    pub(crate) fn maybe_block(&self) -> Option<Box<Block>> {
        match self {
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..) => None,
            BlockAcquisitionState::HaveAllDeploys(block, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _)
            | BlockAcquisitionState::HaveBlock(block, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _) => Some(block.clone()),
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
    // the BlockAcquisitionState states and their valid transitions follow:
    //
    //   Initialized -> need block header
    //
    //   HaveBlockHeader -> if no era validators -> need era validator weights
    //                    else need weak finality
    //
    //   HaveWeakFinalitySignatures -> need block
    //
    //   HaveBlock -> if should_fetch_execution_state -> need global state
    //              else if block has deploys need approvals hashes
    //              else if no deploys need strict finality
    //
    //   HaveGlobalState -> if should_fetch_execution_state
    //                      if block has deploys ->
    //                       if have execution effects -> need approvals hashes
    //                       else -> need execution effects
    //                      else -> need strict finality
    //                     else -> error
    //
    //   HaveAllExecutionResults -> if should_fetch_execution_state
    //                                if approvals checkable -> need approvals hashes
    //                                else -> need deploys
    //                               else error
    //
    //   HaveApprovalsHashes -> need deploys
    //
    //   HaveDeploys -> need strict finality
    //
    //   HaveStrictFinalitySignatures -> HaveStrictFinalitySignatures (success / terminal)
    //
    //   Failed -> Failed (terminal)
    //
    /// Determines what action should be taken to acquire the next needed block related data.
    pub(super) fn next_action(
        &mut self,
        peer_list: &PeerList,
        validator_weights: &EraValidatorWeights,
        rng: &mut NodeRng,
        is_historical: bool,
    ) -> Result<BlockAcquisitionAction, BlockAcquisitionError> {
        // self is the resting state we are in, ret is the next action that should be taken
        // to acquire the necessary data to get us to the next step (if any), or an error
        let ret = match self {
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
            BlockAcquisitionState::HaveBlock(block, signatures, deploys) => {
                if is_historical {
                    Ok(BlockAcquisitionAction::global_state(
                        peer_list,
                        rng,
                        *block.hash(),
                        *block.state_root_hash(),
                    ))
                } else if deploys.needs_deploy().is_some() {
                    Ok(BlockAcquisitionAction::approvals_hashes(
                        block, peer_list, rng,
                    ))
                } else {
                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                        is_historical,
                    ))
                }
            }
            BlockAcquisitionState::HaveGlobalState(
                block,
                signatures,
                deploy_state,
                exec_results,
            ) => {
                if is_historical == false {
                    Err(BlockAcquisitionError::InvalidStateTransition)
                } else if deploy_state.needs_deploy().is_some() {
                    BlockAcquisitionAction::maybe_execution_results(
                        block,
                        peer_list,
                        rng,
                        exec_results,
                    )
                } else {
                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                        is_historical,
                    ))
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(
                block,
                signatures,
                deploys,
                checksum,
            ) => match (is_historical, checksum) {
                (false, _) => Err(BlockAcquisitionError::InvalidStateTransition),
                (true, ExecutionResultsChecksum::Checkable(_)) => Ok(
                    BlockAcquisitionAction::approvals_hashes(block, peer_list, rng),
                ),
                (true, ExecutionResultsChecksum::Uncheckable) => {
                    Ok(BlockAcquisitionAction::maybe_needs_deploy(
                        block.header(),
                        peer_list,
                        rng,
                        validator_weights,
                        signatures,
                        deploys.needs_deploy(),
                        is_historical,
                    ))
                }
            },
            BlockAcquisitionState::HaveApprovalsHashes(block, signatures, deploys) => {
                Ok(BlockAcquisitionAction::maybe_needs_deploy(
                    block.header(),
                    peer_list,
                    rng,
                    validator_weights,
                    signatures,
                    deploys.needs_deploy(),
                    is_historical,
                ))
            }
            BlockAcquisitionState::HaveAllDeploys(block, signatures) => {
                Ok(BlockAcquisitionAction::strict_finality_signatures(
                    peer_list,
                    rng,
                    block.header(),
                    validator_weights,
                    signatures,
                    is_historical,
                ))
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, ..) => {
                Ok(BlockAcquisitionAction::need_nothing(block.id()))
            }
            BlockAcquisitionState::Failed(block_hash, ..) => {
                Ok(BlockAcquisitionAction::need_nothing(*block_hash))
            }
        };
        ret
    }

    /// The block height of the current block, if available.
    pub(super) fn block_height(&self) -> Option<u64> {
        match self {
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Failed(..) => None,
            BlockAcquisitionState::HaveBlockHeader(header, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) => Some(header.height()),
            BlockAcquisitionState::HaveBlock(block, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _)
            | BlockAcquisitionState::HaveAllDeploys(block, ..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => Some(block.height()),
        }
    }

    /// Register the block header for this block.
    pub(super) fn register_block_header(
        &mut self,
        header: BlockHeader,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        let new_state = match self {
            BlockAcquisitionState::Initialized(block_hash, signatures) => {
                if header.id() == *block_hash {
                    info!("BlockAcquisition: registering header for: {}", block_hash);
                    BlockAcquisitionState::HaveBlockHeader(Box::new(header), signatures.clone())
                } else {
                    return Err(BlockAcquisitionError::BlockHashMismatch {
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
            | BlockAcquisitionState::Failed(..) => return Ok(None),
        };
        self.set_state(new_state);
        Ok(Some(Acceptance::NeededIt))
    }

    /// Register the block body for this block.
    pub(super) fn register_block(
        &mut self,
        block: &Block,
        need_execution_state: bool,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        let new_state = match self {
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, signatures) => {
                let expected_block_hash = header.block_hash();
                let actual_block_hash = block.hash();
                if *actual_block_hash != expected_block_hash {
                    return Err(BlockAcquisitionError::BlockHashMismatch {
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(None);
            }
        };
        self.set_state(new_state);
        Ok(Some(Acceptance::NeededIt))
    }

    /// Register a finality signature for this block.
    pub(super) fn register_finality_signature(
        &mut self,
        signature: FinalitySignature,
        validator_weights: &EraValidatorWeights,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        // we will accept finality signatures we don't yet have while in every state other than
        // Initialized and Failed. However, it can only cause a state transition when we
        // are in a resting state that needs weak finality or strict finality.

        let signer = signature.public_key.clone();
        let added: bool;
        let maybe_block_hash: Option<BlockHash>;
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
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveGlobalState(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveApprovalsHashes(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, acquired_signatures) => {
                maybe_block_hash = Some(block.id());
                added = acquired_signatures.apply_signature(signature);
                None
            }
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, acquired_signatures) => {
                // 1: In HaveWeakFinalitySignatures we are waiting to acquire the block body
                // 2: In HaveStrictFinalitySignatures we are in the happy path resting state
                // and have enough signatures, but not necessarily all signatures and
                // will accept late comers while resting in this state
                maybe_block_hash = Some(header.block_hash());
                added = acquired_signatures.apply_signature(signature);
                None
            }
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Failed(..) => {
                return Ok(None)
            }
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
        Ok(Some(ret))
    }

    /// Register the approvals hashes for this block.
    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        need_execution_state: bool,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(None);
            }
        };
        self.set_state(new_state);
        Ok(Some(Acceptance::NeededIt))
    }

    /// Register global state for this block.
    pub(super) fn register_global_state(
        &mut self,
        root_hash: Digest,
        need_execution_state: bool,
    ) -> Result<(), BlockAcquisitionError> {
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
                    return Err(BlockAcquisitionError::RootHashMismatch {
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
    }

    /// Register execution results checksum for this block.
    pub(super) fn register_execution_results_checksum(
        &mut self,
        execution_results_checksum: ExecutionResultsChecksum,
        need_execution_state: bool,
    ) -> Result<(), BlockAcquisitionError> {
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
                    .map_err(BlockAcquisitionError::ExecutionResults)?;
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(());
            }
        };
        Ok(())
    }

    /// Register execution results or chunk for this block.
    pub(super) fn register_execution_results_or_chunk(
        &mut self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
        need_execution_state: bool,
    ) -> Result<Option<HashMap<DeployHash, casper_types::ExecutionResult>>, BlockAcquisitionError>
    {
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
                let deploy_hashes = block.deploy_and_transfer_hashes().copied().collect();
                match exec_results_acq
                    .clone()
                    .apply_block_execution_results_or_chunk(
                        block_execution_results_or_chunk,
                        deploy_hashes,
                    ) {
                    Ok(new_effects) => match new_effects {
                        ExecutionResultsAcquisition::Needed { .. }
                        | ExecutionResultsAcquisition::Pending { .. } => return Ok(None),
                        ExecutionResultsAcquisition::Complete { ref results, .. } => (
                            BlockAcquisitionState::HaveGlobalState(
                                block.clone(),
                                signatures.clone(),
                                deploys.clone(),
                                new_effects.clone(),
                            ),
                            Some(results.clone()),
                        ),
                        ExecutionResultsAcquisition::Acquiring { .. } => (
                            BlockAcquisitionState::HaveGlobalState(
                                block.clone(),
                                signatures.clone(),
                                deploys.clone(),
                                new_effects,
                            ),
                            None,
                        ),
                    },
                    Err(error) => {
                        warn!(%error, "failed to apply execution results");
                        return Err(BlockAcquisitionError::ExecutionResults(error));
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(None);
            }
        };
        self.set_state(new_state);
        Ok(ret)
    }

    /// Register execution results stored notification for this block.
    pub(super) fn register_execution_results_stored_notification(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), BlockAcquisitionError> {
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
    }

    /// Register a deploy for this block.
    pub(super) fn register_deploy(
        &mut self,
        deploy_id: DeployId,
        is_historical: bool,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        let (block, signatures, deploys) = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, deploys)
                if false == is_historical =>
            {
                (block, signatures, deploys)
            }
            BlockAcquisitionState::HaveApprovalsHashes(block, signatures, deploys) => {
                (block, signatures, deploys)
            }
            BlockAcquisitionState::HaveAllExecutionResults(
                block,
                signatures,
                deploys,
                checksum,
            ) if is_historical => match checksum {
                ExecutionResultsChecksum::Uncheckable => (block, signatures, deploys),
                ExecutionResultsChecksum::Checkable(_) => {
                    return Err(BlockAcquisitionError::InvalidAttemptToApplyDeploy { deploy_id });
                }
            },
            BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveBlockHeader(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => {
                warn!(
                    ?deploy_id,
                    "BlockAcquisition: invalid attempt to register deploy for: {}",
                    self.block_hash()
                );
                return Ok(None);
            }
        };
        info!("BlockAcquisition: registering deploy for: {}", block.id());
        let maybe_acceptance = deploys.apply_deploy(deploy_id);
        if deploys.needs_deploy().is_none() {
            let new_state =
                BlockAcquisitionState::HaveAllDeploys(block.clone(), signatures.clone());
            self.set_state(new_state);
        }
        Ok(maybe_acceptance)
    }

    /// Register block executed for this block.
    pub(super) fn register_block_execution_enqueued(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), BlockAcquisitionError> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, deploy_acquisition)
                if !need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering block execution for: {}",
                    block.id()
                );
                if deploy_acquisition.needs_deploy().is_some() {
                    return Err(BlockAcquisitionError::InvalidAttemptToEnqueueBlockForExecution);
                }
                // this must be a block w/ no deploys and thus also no approvals hashes;
                // we can go straight to strict finality
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
                    acquired_signatures.clone(),
                )
            }
            BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures) => {
                info!(
                    "BlockAcquisition: registering block execution for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
    }

    /// Register marked complete (all required data stored locally) for this block.
    pub(super) fn register_marked_complete(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), BlockAcquisitionError> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, deploy_acquisition)
                if !need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.id()
                );
                if deploy_acquisition.needs_deploy().is_some() {
                    return Err(BlockAcquisitionError::InvalidAttemptToMarkComplete);
                }
                // this must be a block w/ no deploys and thus also no execution effects or
                // approvals hashes; we can go straight to strict finality
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
                    acquired_signatures.clone(),
                )
            }
            BlockAcquisitionState::HaveGlobalState(block, acquired_signatures, ..)
                if need_execution_state =>
            {
                // this must be a block w/ no deploys and thus also no execution effects or
                // approvals hashes; we can go straight to strict finality
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
                    acquired_signatures.clone(),
                )
            }
            BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures) => {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.id()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
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
            | BlockAcquisitionState::Failed(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
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
