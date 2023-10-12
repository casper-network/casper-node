use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use derive_more::Display;
use tracing::{debug, error, info, trace, warn};

use casper_types::{
    execution::ExecutionResult, Block, BlockHash, BlockHeader, DeployHash, DeployId, Digest, EraId,
    FinalitySignature, ProtocolVersion, PublicKey,
};

use crate::{
    components::block_synchronizer::{
        block_acquisition_action::BlockAcquisitionAction, deploy_acquisition::DeployAcquisition,
        peer_list::PeerList, signature_acquisition::SignatureAcquisition, BlockAcquisitionError,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{
        ApprovalsHashes, BlockExecutionResultsOrChunk, EraValidatorWeights, ExecutableBlock,
        SignatureWeight,
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
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     Initialized --> HaveBlockHeader
///     HaveBlockHeader --> HaveWeakFinalitySignatures
///     HaveWeakFinalitySignatures --> HaveBlock
///     HaveBlock --> B{is historical?}
///     B -->|Yes| HaveGlobalState
///     B -->|No| C
///     HaveGlobalState --> HaveAllExecutionResults
///     HaveAllExecutionResults --> A{is legacy block?}
///     A -->|Yes| C
///     A -->|No| HaveApprovalsHashes
///     HaveApprovalsHashes --> C{is block empty?}
///     C -->|Yes| HaveStrictFinalitySignatures
///     C -->|No| HaveAllDeploys
///     HaveAllDeploys --> HaveStrictFinalitySignatures
///     HaveStrictFinalitySignatures --> D{is historical?}
///     D -->|Yes| Complete
///     D -->|No| HaveFinalizedBlock
///     HaveFinalizedBlock --> Complete
/// ```
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
    // We keep the `Block` as well as the `FinalizedBlock` because the
    // block is necessary to reach the `Complete` state and the finalized
    // block is used to enqueue for execution. While the block would surely
    // be stored by the time we get to this state, it would be inefficient
    // to fetch it from storage again to transition to the `Complete` state,
    // so it is retained. The downside is that the block is useful in its
    // entirety only in the historical sync, and `HaveFinalizedBlock` along
    // with execution are strictly forward sync states. Until a refactor splits
    // the `Complete` states for the historical and forward cases, we need to
    // keep the block around.
    HaveFinalizedBlock(Box<Block>, Box<ExecutableBlock>, bool),
    // The `Complete` state needs the block itself in order to produce a meta
    // block announcement in the historical sync flow. In the forward sync,
    // only the block hash and height are necessary. Therefore, we retain the
    // block fully in this state.
    Complete(Box<Block>),
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
                block.height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveGlobalState(block, _, _, _) => write!(
                f,
                "have global state({}) for: {}",
                block.height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _) => write!(
                f,
                "have execution results({}) for: {}",
                block.height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveApprovalsHashes(block, _, _) => write!(
                f,
                "have approvals hashes({}) for: {}",
                block.height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveAllDeploys(block, _) => {
                write!(f, "have deploys({}) for: {}", block.height(), block.hash())
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => write!(
                f,
                "have strict finality({}) for: {}",
                block.height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveFinalizedBlock(block, _, _) => write!(
                f,
                "have finalized block({}) for: {}",
                block.height(),
                *block.hash()
            ),
            BlockAcquisitionState::Complete(block) => {
                write!(
                    f,
                    "have complete block({}) for: {}",
                    block.height(),
                    *block.hash()
                )
            }
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
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _)
            | BlockAcquisitionState::HaveFinalizedBlock(block, ..)
            | BlockAcquisitionState::Complete(block) => *block.hash(),
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
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _)
            | BlockAcquisitionState::HaveFinalizedBlock(block, _, _)
            | BlockAcquisitionState::Complete(block) => Some(block.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, Display, PartialEq)]
#[must_use]
pub(super) enum Acceptance {
    #[display(fmt = "had it")]
    HadIt,
    #[display(fmt = "needed it")]
    NeededIt,
}

pub(super) struct RegisterExecResultsOutcome {
    pub(super) exec_results: Option<HashMap<DeployHash, ExecutionResult>>,
    pub(super) acceptance: Option<Acceptance>,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// sequenceDiagram
///     Note right of Initialized: need next
///     Initialized ->> BlockHeader: get header
///     BlockHeader ->> WeakFinalitySignatures: get at least weak finality
///     WeakFinalitySignatures ->> Block: get block
///     Block -->> GlobalState: is historical?
///     GlobalState ->> AllExecutionResults: get execution results
///     AllExecutionResults -->> ApprovalsHashes: is not legacy?
///     AllExecutionResults -->> AllDeploys: is legacy?
///     ApprovalsHashes ->> AllDeploys: get deploys
///     GlobalState -->> StrictFinalitySignatures: is block empty?
///     Block -->> AllDeploys: is not historical and is not empty?
///     Block -->> StrictFinalitySignatures: is not historical and is empty?
///     AllDeploys ->> StrictFinalitySignatures: get strict finality
///     StrictFinalitySignatures ->> FinalizedBlock: is forward and finalized block created
///     StrictFinalitySignatures -->> Complete: is historical and block marked complete
///     FinalizedBlock ->> Complete: is forward and block executed
/// ```
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
    //   HaveStrictFinalitySignatures -> if should_fetch_execution_state -> need to mark block
    // complete                                else need to convert block to FinalizedBlock
    //
    //   HaveFinalizedBlock -> need enqueue block for execution
    //
    //   Complete -> Complete (success / terminal)
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
        max_simultaneous_peers: u8,
    ) -> Result<BlockAcquisitionAction, BlockAcquisitionError> {
        // self is the resting state we are in, ret is the next action that should be taken
        // to acquire the necessary data to get us to the next step (if any), or an error
        let ret = match self {
            BlockAcquisitionState::Initialized(block_hash, ..) => Ok(
                BlockAcquisitionAction::block_header(peer_list, rng, *block_hash),
            ),
            BlockAcquisitionState::HaveBlockHeader(block_header, signatures) => {
                Ok(signatures_from_missing_validators(
                    validator_weights,
                    signatures,
                    max_simultaneous_peers,
                    peer_list,
                    rng,
                    block_header.era_id(),
                    block_header.block_hash(),
                ))
            }
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) => Ok(
                BlockAcquisitionAction::block_body(peer_list, rng, header.block_hash()),
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
                } else if signatures.has_sufficient_finality(is_historical, true) {
                    Ok(BlockAcquisitionAction::switch_to_have_sufficient_finality(
                        *block.hash(),
                        block.height(),
                    ))
                } else {
                    Ok(signatures_from_missing_validators(
                        validator_weights,
                        signatures,
                        max_simultaneous_peers,
                        peer_list,
                        rng,
                        block.era_id(),
                        *block.hash(),
                    ))
                }
            }
            BlockAcquisitionState::HaveGlobalState(
                block,
                signatures,
                deploy_state,
                exec_results,
            ) => {
                if false == is_historical {
                    Err(BlockAcquisitionError::InvalidStateTransition)
                } else if deploy_state.needs_deploy().is_some() {
                    BlockAcquisitionAction::maybe_execution_results(
                        block,
                        peer_list,
                        rng,
                        exec_results,
                    )
                } else if signatures.has_sufficient_finality(is_historical, true) {
                    Ok(BlockAcquisitionAction::switch_to_have_sufficient_finality(
                        *block.hash(),
                        block.height(),
                    ))
                } else {
                    Ok(signatures_from_missing_validators(
                        validator_weights,
                        signatures,
                        max_simultaneous_peers,
                        peer_list,
                        rng,
                        block.era_id(),
                        *block.hash(),
                    ))
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(
                block,
                signatures,
                deploys,
                checksum,
            ) if is_historical => {
                let is_checkable = checksum.is_checkable();
                signatures.set_is_legacy(!is_checkable);
                if is_checkable {
                    Ok(BlockAcquisitionAction::approvals_hashes(
                        block, peer_list, rng,
                    ))
                } else {
                    Ok(BlockAcquisitionAction::maybe_needs_deploy(
                        *block.hash(),
                        block.height(),
                        block.era_id(),
                        peer_list,
                        rng,
                        validator_weights,
                        signatures,
                        deploys.needs_deploy(),
                        is_historical,
                        max_simultaneous_peers,
                    ))
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _) => {
                Err(BlockAcquisitionError::InvalidStateTransition)
            }
            BlockAcquisitionState::HaveApprovalsHashes(block, signatures, deploys) => {
                Ok(BlockAcquisitionAction::maybe_needs_deploy(
                    *block.hash(),
                    block.height(),
                    block.era_id(),
                    peer_list,
                    rng,
                    validator_weights,
                    signatures,
                    deploys.needs_deploy(),
                    is_historical,
                    max_simultaneous_peers,
                ))
            }
            BlockAcquisitionState::HaveAllDeploys(block, signatures) => {
                if signatures.has_sufficient_finality(is_historical, true) {
                    Ok(BlockAcquisitionAction::switch_to_have_sufficient_finality(
                        *block.hash(),
                        block.height(),
                    ))
                } else {
                    Ok(signatures_from_missing_validators(
                        validator_weights,
                        signatures,
                        max_simultaneous_peers,
                        peer_list,
                        rng,
                        block.era_id(),
                        *block.hash(),
                    ))
                }
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, ..) => {
                if is_historical {
                    // we have enough signatures; need to make sure we've stored the necessary bits
                    Ok(BlockAcquisitionAction::block_marked_complete(
                        *block.hash(),
                        block.height(),
                    ))
                } else {
                    Ok(BlockAcquisitionAction::make_executable_block(
                        *block.hash(),
                        block.height(),
                    ))
                }
            }
            BlockAcquisitionState::HaveFinalizedBlock(block, executable_block, enqueued) => {
                if is_historical {
                    Err(BlockAcquisitionError::InvalidStateTransition)
                } else if *enqueued == false {
                    Ok(BlockAcquisitionAction::enqueue_block_for_execution(
                        block.hash(),
                        executable_block.clone(),
                    ))
                } else {
                    // if the block was already enqueued for execution just wait, there's
                    // nothing else to do
                    Ok(BlockAcquisitionAction::need_nothing(*block.hash()))
                }
            }
            BlockAcquisitionState::Complete(block) => {
                Ok(BlockAcquisitionAction::need_nothing(*block.hash()))
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
            BlockAcquisitionState::HaveFinalizedBlock(_, executable_block, _) => {
                Some(executable_block.height)
            }
            BlockAcquisitionState::HaveBlock(block, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _)
            | BlockAcquisitionState::HaveAllDeploys(block, ..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _)
            | BlockAcquisitionState::Complete(block) => Some(block.height()),
        }
    }

    /// Register the block header for this block.
    pub(super) fn register_block_header(
        &mut self,
        header: BlockHeader,
        strict_finality_protocol_version: ProtocolVersion,
        is_historical: bool,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        let new_state = match self {
            BlockAcquisitionState::Initialized(block_hash, signatures) => {
                if header.block_hash() == *block_hash {
                    info!(
                        "BlockAcquisition: registering header for: {:?}, height: {}",
                        block_hash,
                        header.height()
                    );
                    let is_legacy_block = is_historical
                        && header.protocol_version() < strict_finality_protocol_version;
                    signatures.set_is_legacy(is_legacy_block);
                    BlockAcquisitionState::HaveBlockHeader(Box::new(header), signatures.clone())
                } else {
                    return Err(BlockAcquisitionError::BlockHashMismatch {
                        expected: *block_hash,
                        actual: header.block_hash(),
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => return Ok(None),
        };
        self.set_state(new_state);
        Ok(Some(Acceptance::NeededIt))
    }

    /// Register the block body for this block.
    pub(super) fn register_block(
        &mut self,
        block: Block,
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
                    Box::new(block),
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
                return Ok(None);
            }
        };
        self.set_state(new_state);
        Ok(Some(Acceptance::NeededIt))
    }

    /// Advance acquisition state to HaveStrictFinality.
    pub(super) fn switch_to_have_strict_finality(
        &mut self,
        block_hash: BlockHash,
        is_historical: bool,
    ) -> Result<(), BlockAcquisitionError> {
        if block_hash != self.block_hash() {
            return Err(BlockAcquisitionError::BlockHashMismatch {
                expected: self.block_hash(),
                actual: block_hash,
            });
        }
        let maybe_new_state = match self {
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveGlobalState(block, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures)
            | BlockAcquisitionState::HaveApprovalsHashes(block, acquired_signatures, ..) => {
                if acquired_signatures.has_sufficient_finality(is_historical, true) {
                    Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                        block.clone(),
                        acquired_signatures.clone(),
                    ))
                } else {
                    return Err(BlockAcquisitionError::InvalidStateTransition);
                }
            }
            BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Complete(..) => None,
        };
        if let Some(new_state) = maybe_new_state {
            self.set_state(new_state);
        };
        Ok(())
    }

    /// Register a finality signature as pending for this block.
    pub(super) fn register_finality_signature_pending(&mut self, validator: PublicKey) {
        match self {
            BlockAcquisitionState::HaveBlockHeader(_, acquired_signatures)
            | BlockAcquisitionState::HaveBlock(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveGlobalState(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveApprovalsHashes(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllDeploys(_, acquired_signatures)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, acquired_signatures)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, acquired_signatures) => {
                acquired_signatures.register_pending(validator);
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {}
        };
    }

    pub(super) fn actively_acquiring_signatures(&self, is_historical: bool) -> bool {
        match self {
            BlockAcquisitionState::HaveBlockHeader(..) => true,
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => false,
            BlockAcquisitionState::HaveBlock(_, acquired_signatures, acquired_deploys) => {
                !is_historical
                    && acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict
            }
            BlockAcquisitionState::HaveGlobalState(
                _,
                acquired_signatures,
                acquired_deploys,
                ..,
            ) => {
                acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict
            }
            BlockAcquisitionState::HaveApprovalsHashes(
                _,
                acquired_signatures,
                acquired_deploys,
            ) => {
                acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict
            }
            BlockAcquisitionState::HaveAllExecutionResults(
                _,
                acquired_signatures,
                acquired_deploys,
                ..,
            ) => {
                acquired_signatures.is_legacy()
                    && acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict
            }
            BlockAcquisitionState::HaveAllDeploys(_, acquired_signatures) => {
                acquired_signatures.signature_weight() != SignatureWeight::Strict
            }
        }
    }

    /// Register a finality signature for this block.
    pub(super) fn register_finality_signature(
        &mut self,
        signature: FinalitySignature,
        validator_weights: &EraValidatorWeights,
        is_historical: bool,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        // we will accept finality signatures we don't yet have while in every state other than
        // Initialized and Failed. However, it can only cause a state transition when we
        // are in a resting state that needs weak finality or strict finality.
        let cloned_sig = signature.clone();
        let signer = signature.public_key().clone();
        let acceptance: Acceptance;
        let maybe_block_hash: Option<BlockHash>;
        let currently_acquiring_sigs = self.actively_acquiring_signatures(is_historical);
        let maybe_new_state: Option<BlockAcquisitionState> = match self {
            BlockAcquisitionState::HaveBlockHeader(header, acquired_signatures) => {
                // we are attempting to acquire at least ~1/3 signature weight before
                // committing to doing non-trivial work to acquire this block
                // thus the primary thing we are doing in this state is accumulating sigs.
                // We also want to ensure we've tried at least once to fetch every potential
                // signature.
                maybe_block_hash = Some(header.block_hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                if acquired_signatures.has_sufficient_finality(is_historical, false) {
                    Some(BlockAcquisitionState::HaveWeakFinalitySignatures(
                        header.clone(),
                        acquired_signatures.clone(),
                    ))
                } else {
                    None
                }
            }
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, acquired_deploys) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                if !is_historical
                    && acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.has_sufficient_finality(is_historical, true)
                {
                    // When syncing a forward block, if we don't need deploys and have all required
                    // signatures, advance the state
                    Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                        block.clone(),
                        acquired_signatures.clone(),
                    ))
                } else {
                    // Otherwise stay in HaveBlock to allow fetching for the next bit of data
                    None
                }
            }
            BlockAcquisitionState::HaveGlobalState(
                block,
                acquired_signatures,
                acquired_deploys,
                ..,
            ) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                if acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.has_sufficient_finality(is_historical, true)
                {
                    Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                        block.clone(),
                        acquired_signatures.clone(),
                    ))
                } else {
                    None
                }
            }
            BlockAcquisitionState::HaveApprovalsHashes(block, acquired_signatures, ..) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                None
            }
            BlockAcquisitionState::HaveAllExecutionResults(
                block,
                acquired_signatures,
                acquired_deploys,
                ..,
            ) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                if acquired_signatures.is_legacy()
                    && acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.has_sufficient_finality(is_historical, true)
                {
                    Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                        block.clone(),
                        acquired_signatures.clone(),
                    ))
                } else {
                    None
                }
            }
            BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                if acquired_signatures.has_sufficient_finality(is_historical, true) {
                    Some(BlockAcquisitionState::HaveStrictFinalitySignatures(
                        block.clone(),
                        acquired_signatures.clone(),
                    ))
                } else {
                    None
                }
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, acquired_signatures) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                None
            }
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, acquired_signatures) => {
                // 1: In HaveWeakFinalitySignatures we are waiting to acquire the block body
                // 2: In HaveStrictFinalitySignatures we are in the happy path resting state
                // and have enough signatures, but not necessarily all signatures and
                // will accept late comers while resting in this state
                maybe_block_hash = Some(header.block_hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                None
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => return Ok(None),
        };
        let ret = currently_acquiring_sigs.then_some(acceptance);
        info!(
            signature=%cloned_sig,
            ?ret,
            "BlockAcquisition: registering finality signature for: {}",
            if let Some(block_hash) = maybe_block_hash {
                block_hash.to_string()
            } else {
                "unknown block".to_string()
            }
        );
        self.log_finality_signature_acceptance(&maybe_block_hash, &signer, ret);
        if let Some(new_state) = maybe_new_state {
            self.set_state(new_state);
        }
        Ok(ret)
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
                    block.hash()
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
                    block.hash()
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
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
                    block.hash()
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
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
        debug!(state=%self, need_execution_state, "BlockAcquisitionState: register_execution_results_checksum");
        match self {
            BlockAcquisitionState::HaveGlobalState(
                block,
                _,
                _,
                acq @ ExecutionResultsAcquisition::Needed { .. },
            ) if need_execution_state => {
                info!(
                    "BlockAcquisition: registering execution results hash for: {}",
                    block.hash()
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
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
    ) -> Result<RegisterExecResultsOutcome, BlockAcquisitionError> {
        debug!(state=%self, need_execution_state,
            block_execution_results_or_chunk=%block_execution_results_or_chunk,
            "register_execution_results_or_chunk");
        let (new_state, maybe_exec_results, acceptance) = match self {
            BlockAcquisitionState::HaveGlobalState(
                block,
                signatures,
                deploys,
                exec_results_acq,
            ) if need_execution_state => {
                info!(
                    "BlockAcquisition: registering execution result or chunk for: {}",
                    block.hash()
                );
                let deploy_hashes = block.deploy_and_transfer_hashes().copied().collect();
                match exec_results_acq
                    .clone()
                    .apply_block_execution_results_or_chunk(
                        block_execution_results_or_chunk,
                        deploy_hashes,
                    ) {
                    Ok((new_acquisition, acceptance)) => match new_acquisition {
                        ExecutionResultsAcquisition::Needed { .. }
                        | ExecutionResultsAcquisition::Pending { .. } => {
                            debug!("apply_block_execution_results_or_chunk: Needed | Pending");
                            return Ok(RegisterExecResultsOutcome {
                                exec_results: None,
                                acceptance: Some(acceptance),
                            });
                        }
                        ExecutionResultsAcquisition::Complete { ref results, .. } => {
                            debug!("apply_block_execution_results_or_chunk: Complete");
                            let new_state = BlockAcquisitionState::HaveGlobalState(
                                block.clone(),
                                signatures.clone(),
                                deploys.clone(),
                                new_acquisition.clone(),
                            );
                            let maybe_exec_results = Some(results.clone());
                            (new_state, maybe_exec_results, acceptance)
                        }
                        ExecutionResultsAcquisition::Acquiring { .. } => {
                            debug!("apply_block_execution_results_or_chunk: Acquiring");
                            let new_state = BlockAcquisitionState::HaveGlobalState(
                                block.clone(),
                                signatures.clone(),
                                deploys.clone(),
                                new_acquisition,
                            );
                            let maybe_exec_results = None;
                            (new_state, maybe_exec_results, acceptance)
                        }
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
                return Ok(RegisterExecResultsOutcome {
                    exec_results: None,
                    acceptance: None,
                });
            }
        };
        self.set_state(new_state);
        Ok(RegisterExecResultsOutcome {
            exec_results: maybe_exec_results,
            acceptance: Some(acceptance),
        })
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
                    block.hash()
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
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
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _)
            | BlockAcquisitionState::Complete(..) => {
                warn!(
                    ?deploy_id,
                    "BlockAcquisition: invalid attempt to register deploy for: {}",
                    self.block_hash()
                );
                return Ok(None);
            }
        };
        info!("BlockAcquisition: registering deploy for: {}", block.hash());
        let maybe_acceptance = deploys.apply_deploy(deploy_id);
        if deploys.needs_deploy().is_none() {
            let new_state =
                BlockAcquisitionState::HaveAllDeploys(block.clone(), signatures.clone());
            self.set_state(new_state);
        }
        Ok(maybe_acceptance)
    }

    pub(super) fn register_made_finalized_block(
        &mut self,
        need_execution_state: bool,
        executable_block: ExecutableBlock,
    ) -> Result<(), BlockAcquisitionError> {
        if need_execution_state {
            return Err(BlockAcquisitionError::InvalidAttemptToEnqueueBlockForExecution);
        }

        let new_state = match self {
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => {
                BlockAcquisitionState::HaveFinalizedBlock(
                    block.clone(),
                    Box::new(executable_block),
                    false,
                )
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
                return Ok(());
            }
        };
        self.set_state(new_state);
        Ok(())
    }

    /// Register block is enqueued for execution with the contract runtime.
    pub(super) fn register_block_execution_enqueued(
        &mut self,
    ) -> Result<(), BlockAcquisitionError> {
        match self {
            BlockAcquisitionState::HaveFinalizedBlock(block, _, enqueued) => {
                info!(
                    "BlockAcquisition: registering block enqueued for execution for: {}",
                    block
                );
                *enqueued = true;
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {}
        };
        Ok(())
    }

    /// Register block executed for this block.
    pub(super) fn register_block_executed(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), BlockAcquisitionError> {
        if need_execution_state {
            return Err(BlockAcquisitionError::InvalidAttemptToEnqueueBlockForExecution);
        }

        let new_state = match self {
            BlockAcquisitionState::HaveFinalizedBlock(block, _, _) => {
                info!(
                    "BlockAcquisition: registering block executed for: {}",
                    *block.hash()
                );
                BlockAcquisitionState::Complete(block.clone())
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
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
        if !need_execution_state {
            return Err(BlockAcquisitionError::InvalidAttemptToMarkComplete);
        }

        let new_state = match self {
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    *block.hash()
                );
                BlockAcquisitionState::Complete(block.clone())
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => {
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
        acceptance: Option<Acceptance>,
    ) {
        match maybe_block_hash {
            None => {
                error!(
                    "BlockAcquisition: unknown block_hash for finality signature from {}",
                    signer
                );
            }
            Some(block_hash) => match acceptance {
                Some(Acceptance::HadIt) => {
                    trace!(
                        "BlockAcquisition: existing finality signature for {:?} from {}",
                        block_hash,
                        signer
                    );
                }
                Some(Acceptance::NeededIt) => {
                    debug!(
                        "BlockAcquisition: new finality signature for {:?} from {}",
                        block_hash, signer
                    );
                }
                None => {
                    debug!(
                        "BlockAcquisition: finality signature for {:?} from {} while not actively \
                        trying to acquire finality signatures",
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

// Collect signatures with Vacant state or which are currently missing from
// the SignatureAcquisition.
pub(super) fn signatures_from_missing_validators(
    validator_weights: &EraValidatorWeights,
    signatures: &mut SignatureAcquisition,
    max_simultaneous_peers: u8,
    peer_list: &PeerList,
    rng: &mut NodeRng,
    era_id: EraId,
    block_hash: BlockHash,
) -> BlockAcquisitionAction {
    let mut missing_signatures_in_random_order: HashSet<PublicKey> = validator_weights
        .missing_validators(signatures.not_vacant())
        .cloned()
        .collect();
    // If there are too few, retry any in Pending state.
    if (missing_signatures_in_random_order.len() as u8) < max_simultaneous_peers {
        missing_signatures_in_random_order.extend(
            validator_weights
                .missing_validators(signatures.not_pending())
                .cloned(),
        );
    }
    BlockAcquisitionAction::finality_signatures(
        peer_list,
        rng,
        era_id,
        block_hash,
        missing_signatures_in_random_order.into_iter().collect(),
    )
}
