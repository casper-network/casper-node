use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display, Formatter},
};

use casper_execution_engine::{core::engine_state, storage::trie::TrieRaw};
use datasize::DataSize;
use derive_more::Display;
use tracing::{debug, error, info, trace, warn};

use casper_hashing::Digest;
use casper_types::{system::auction::EraValidators, ProtocolVersion, PublicKey};

use crate::{
    components::block_synchronizer::{
        block_acquisition_action::BlockAcquisitionAction, deploy_acquisition::DeployAcquisition,
        peer_list::PeerList, signature_acquisition::SignatureAcquisition, BlockAcquisitionError,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{
        chainspec::LegacyRequiredFinality, ApprovalsHashes, Block, BlockExecutionResultsOrChunk,
        BlockHash, BlockHeader, DeployHash, DeployId, EraValidatorWeights, FinalitySignature,
        SignatureWeight, TrieOrChunk,
    },
    NodeRng,
};

use super::{
    era_validators_acquisition::{
        EraValidatorsAcquisition, EraValidatorsAcquisitionState,
        Error as EraValidatorsAcquisitionError,
    },
    event::EraValidatorsGetError,
    global_state_acquisition::{Error as GlobalStateAcquisitionError, GlobalStateAcquisition},
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
/// ```
#[derive(Clone, DataSize, Debug)]
pub(super) enum BlockAcquisitionState {
    Initialized(BlockHash, SignatureAcquisition),
    HaveBlockHeader {
        header: Box<BlockHeader>,
        acquired_signatures: SignatureAcquisition,
        acquired_block_era_validators: EraValidatorsAcquisition,
        acquired_parent_block_era_validators: EraValidatorsAcquisition,
        parent_block_header: Option<BlockHeader>,
    },
    HaveWeakFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    HaveBlock(
        Box<Block>,
        SignatureAcquisition,
        DeployAcquisition,
        Option<GlobalStateAcquisition>,
    ),
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
            BlockAcquisitionState::HaveBlockHeader {
                header: block_header,
                ..
            } => write!(
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
            BlockAcquisitionState::HaveBlock(block, _, _, _) => write!(
                f,
                "have block body({}) for: {}",
                block.header().height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveGlobalState(block, _, _, _) => write!(
                f,
                "have global state({}) for: {}",
                block.header().height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _) => write!(
                f,
                "have execution results({}) for: {}",
                block.header().height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveApprovalsHashes(block, _, _) => write!(
                f,
                "have approvals hashes({}) for: {}",
                block.header().height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveAllDeploys(block, _) => write!(
                f,
                "have deploys({}) for: {}",
                block.header().height(),
                block.hash()
            ),
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => write!(
                f,
                "have strict finality({}) for: {}",
                block.header().height(),
                block.hash()
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
            BlockAcquisitionState::HaveBlockHeader {
                header: block_header,
                ..
            }
            | BlockAcquisitionState::HaveWeakFinalitySignatures(block_header, _) => {
                block_header.block_hash()
            }
            BlockAcquisitionState::HaveBlock(block, _, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _)
            | BlockAcquisitionState::HaveAllDeploys(block, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _) => *block.hash(),
        }
    }

    pub(crate) fn maybe_block(&self) -> Option<Box<Block>> {
        match self {
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::HaveBlockHeader { .. }
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..) => None,
            BlockAcquisitionState::HaveAllDeploys(block, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(block, _)
            | BlockAcquisitionState::HaveBlock(block, _, _, _)
            | BlockAcquisitionState::HaveGlobalState(block, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(block, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(block, _, _) => Some(block.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, Display)]
#[must_use]
pub(super) enum Acceptance {
    #[display(fmt = "had it")]
    HadIt,
    #[display(fmt = "needed it")]
    NeededIt,
}

pub(super) struct NextActionConfig {
    pub(super) legacy_required_finality: LegacyRequiredFinality,
    pub(super) max_simultaneous_peers: usize,
    pub(super) max_parallel_trie_fetches: usize,
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
    //   HaveStrictFinalitySignatures -> HaveStrictFinalitySignatures (success / terminal)
    //
    //   Failed -> Failed (terminal)
    //
    /// Determines what action should be taken to acquire the next needed block related data.
    pub(super) fn next_action(
        &mut self,
        peer_list: &PeerList,
        validator_weights: Option<&EraValidatorWeights>,
        rng: &mut NodeRng,
        is_historical: bool,
        restrictions: NextActionConfig,
    ) -> Result<BlockAcquisitionAction, BlockAcquisitionError> {
        // self is the resting state we are in, ret is the next action that should be taken
        // to acquire the necessary data to get us to the next step (if any), or an error
        let ret = match self {
            BlockAcquisitionState::Initialized(block_hash, ..) => Ok(
                BlockAcquisitionAction::block_header(peer_list, rng, *block_hash),
            ),
            BlockAcquisitionState::HaveBlockHeader {
                header,
                acquired_signatures,
                acquired_block_era_validators,
                acquired_parent_block_era_validators,
                parent_block_header,
            } => {
                if let Some(validator_weights) = validator_weights {
                    // Collect signatures with Vacant state or which are currently missing from
                    // SignatureAcquisition.
                    Ok(signatures_from_missing_validators(
                        validator_weights,
                        acquired_signatures,
                        restrictions.max_simultaneous_peers,
                        peer_list,
                        rng,
                        header,
                    ))
                } else {
                    let era_id = header.era_id();

                    let block_state_root_hash = *header.state_root_hash();
                    let block_protocol_version = header.protocol_version();

                    let block_era_validators = match acquired_block_era_validators.era_validators()
                    {
                        Ok(era_validators) => era_validators,
                        Err(_) => {
                            return Self::era_validators_acquisition_next_action(
                                acquired_block_era_validators,
                                header.block_hash(),
                                block_state_root_hash,
                                block_protocol_version,
                                rng,
                                peer_list,
                                restrictions.max_parallel_trie_fetches,
                            );
                        }
                    };

                    let parent_era_validators = if let Some(parent_header) = parent_block_header {
                        let parent_block_state_root_hash = *parent_header.state_root_hash();
                        let parent_block_protocol_version = parent_header.protocol_version();

                        match acquired_parent_block_era_validators.era_validators() {
                            Ok(era_validators) => era_validators,
                            Err(_) => {
                                return Self::era_validators_acquisition_next_action(
                                    acquired_parent_block_era_validators,
                                    header.block_hash(),
                                    parent_block_state_root_hash,
                                    parent_block_protocol_version,
                                    rng,
                                    peer_list,
                                    restrictions.max_parallel_trie_fetches,
                                );
                            }
                        }
                    } else {
                        return Ok(BlockAcquisitionAction::get_block_header_from_storage(
                            header.block_hash(),
                            *header.parent_hash(),
                        ));
                    };

                    // We managed to read the era validators from the global states of an immediate
                    // switch block and its parent. We now can check for the signs of any changes
                    // happening during the upgrade and update the validator matrix with the correct
                    // set of validators.
                    // `era_id`, being the era of the immediate switch block, will be absent in the
                    // validators stored in the immediate switch block - therefore we will use its
                    // successor for the comparison.
                    let era_to_check = era_id.successor();
                    // We read the validators for era_id+1 from the parent of the immediate switch
                    // block.
                    let validators_in_parent = match parent_era_validators.get(&era_to_check) {
                        Some(validators) => validators,
                        None => {
                            return Err(BlockAcquisitionError::MissingEraValidatorWeights);
                        }
                    };
                    // We also read the validators from the immediate switch block itself.
                    let validators_in_block = match block_era_validators.get(&era_to_check) {
                        Some(validators) => validators,
                        None => {
                            return Err(BlockAcquisitionError::MissingEraValidatorWeights);
                        }
                    };
                    // Decide which validators to use for `era_id` in the validators matrix.
                    let validators_to_register = if validators_in_parent == validators_in_block {
                        // Nothing interesting happened - register the regular validators, ie. the
                        // ones stored for `era_id` in the parent of the immediate switch block.
                        match parent_era_validators.get(&era_id) {
                            Some(validators) => validators,
                            None => {
                                return Err(BlockAcquisitionError::MissingEraValidatorWeights);
                            }
                        }
                    } else {
                        // We had an upgrade changing the validators! We use the same validators
                        // that will be used for the era after the immediate
                        // switch block, as we can't trust the ones we would
                        // use normally.
                        validators_in_block
                    };

                    return Ok(BlockAcquisitionAction::update_era_validators(
                        era_id,
                        validators_to_register.clone(),
                    ));
                }
            }
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) => Ok(
                BlockAcquisitionAction::block_body(peer_list, rng, header.block_hash()),
            ),
            BlockAcquisitionState::HaveBlock(
                block,
                signatures,
                deploys,
                global_state_acquisition,
            ) => {
                if let Some(global_state_acquisition) = global_state_acquisition {
                    Ok(BlockAcquisitionAction::global_state(
                        peer_list,
                        rng,
                        *block.hash(),
                        *block.state_root_hash(),
                        global_state_acquisition,
                        restrictions.max_parallel_trie_fetches,
                    ))
                } else if deploys.needs_deploy().is_some() {
                    Ok(BlockAcquisitionAction::approvals_hashes(
                        block, peer_list, rng,
                    ))
                } else {
                    let validator_weights = if let Some(validator_weights) = validator_weights {
                        validator_weights
                    } else {
                        return Err(BlockAcquisitionError::MissingEraValidatorWeights);
                    };

                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                        is_historical,
                        restrictions.legacy_required_finality,
                        restrictions.max_simultaneous_peers,
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
                } else {
                    let validator_weights = if let Some(validator_weights) = validator_weights {
                        validator_weights
                    } else {
                        return Err(BlockAcquisitionError::MissingEraValidatorWeights);
                    };

                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                        is_historical,
                        restrictions.legacy_required_finality,
                        restrictions.max_simultaneous_peers,
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
                signatures.set_is_checkable(is_checkable);
                if is_checkable {
                    Ok(BlockAcquisitionAction::approvals_hashes(
                        block, peer_list, rng,
                    ))
                } else {
                    let validator_weights = if let Some(validator_weights) = validator_weights {
                        validator_weights
                    } else {
                        return Err(BlockAcquisitionError::MissingEraValidatorWeights);
                    };

                    Ok(BlockAcquisitionAction::maybe_needs_deploy(
                        block.header(),
                        peer_list,
                        rng,
                        validator_weights,
                        signatures,
                        deploys.needs_deploy(),
                        is_historical,
                        restrictions.legacy_required_finality,
                        restrictions.max_simultaneous_peers,
                    ))
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _) => {
                Err(BlockAcquisitionError::InvalidStateTransition)
            }
            BlockAcquisitionState::HaveApprovalsHashes(block, signatures, deploys) => {
                if let Some(validator_weights) = validator_weights {
                    Ok(BlockAcquisitionAction::maybe_needs_deploy(
                        block.header(),
                        peer_list,
                        rng,
                        validator_weights,
                        signatures,
                        deploys.needs_deploy(),
                        is_historical,
                        restrictions.legacy_required_finality,
                        restrictions.max_simultaneous_peers,
                    ))
                } else {
                    Err(BlockAcquisitionError::MissingEraValidatorWeights)
                }
            }
            BlockAcquisitionState::HaveAllDeploys(block, signatures) => {
                if let Some(validator_weights) = validator_weights {
                    Ok(BlockAcquisitionAction::strict_finality_signatures(
                        peer_list,
                        rng,
                        block.header(),
                        validator_weights,
                        signatures,
                        is_historical,
                        restrictions.legacy_required_finality,
                        restrictions.max_simultaneous_peers,
                    ))
                } else {
                    Err(BlockAcquisitionError::MissingEraValidatorWeights)
                }
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, ..) => {
                Ok(BlockAcquisitionAction::need_nothing(*block.hash()))
            }
            BlockAcquisitionState::Failed(block_hash, ..) => {
                Ok(BlockAcquisitionAction::need_nothing(*block_hash))
            }
        };
        ret
    }

    fn era_validators_acquisition_next_action(
        acquired_era_validators: &mut EraValidatorsAcquisition,
        block_hash: BlockHash,
        block_state_root_hash: Digest,
        block_protocol_version: ProtocolVersion,
        rng: &mut NodeRng,
        peer_list: &PeerList,
        max_parallel_trie_fetches: usize,
    ) -> Result<BlockAcquisitionAction, BlockAcquisitionError> {
        if acquired_era_validators.is_empty() {
            return Ok(
                BlockAcquisitionAction::era_validators_from_contract_runtime(vec![(
                    block_state_root_hash,
                    block_protocol_version,
                )]),
            );
        }

        if acquired_era_validators.is_pending_from_storage() {
            if !acquired_era_validators.is_acquiring_from_root_hash(&block_state_root_hash) {
                return Err(BlockAcquisitionError::RootHashMismatch {
                    expected: block_state_root_hash,
                    actual: acquired_era_validators.state_root_hash().unwrap(),
                });
            }
            return Ok(
                BlockAcquisitionAction::era_validators_from_contract_runtime(vec![(
                    block_state_root_hash,
                    block_protocol_version,
                )]),
            );
        }

        if acquired_era_validators.is_pending_global_state() {
            if !acquired_era_validators.is_acquiring_from_root_hash(&block_state_root_hash) {
                return Err(BlockAcquisitionError::RootHashMismatch {
                    expected: block_state_root_hash,
                    actual: acquired_era_validators.state_root_hash().unwrap(),
                });
            }

            let global_state_acquisition = acquired_era_validators
                .global_state_acquisition_mut()
                .unwrap();
            return Ok(BlockAcquisitionAction::global_state(
                peer_list,
                rng,
                block_hash,
                block_state_root_hash,
                global_state_acquisition,
                max_parallel_trie_fetches,
            ));
        }

        Ok(BlockAcquisitionAction::need_nothing(block_hash))
    }

    pub(super) fn register_pending_put_tries(
        &mut self,
        state_root_hash: Digest,
        put_tries_in_progress: HashSet<Digest>,
    ) {
        match self {
            BlockAcquisitionState::HaveBlock(_, _, _, Some(global_state_acq)) => {
                global_state_acq.register_pending_put_tries(put_tries_in_progress);
            }
            BlockAcquisitionState::HaveBlockHeader {
                acquired_block_era_validators,
                acquired_parent_block_era_validators,
                ..
            } => {
                match (
                    acquired_block_era_validators.is_acquiring_from_root_hash(&state_root_hash),
                    acquired_parent_block_era_validators
                        .is_acquiring_from_root_hash(&state_root_hash),
                ) {
                    (true, false) => {
                        if let EraValidatorsAcquisitionState::PendingGlobalState {
                            global_state_acquisition,
                        } = acquired_block_era_validators.state_mut()
                        {
                            global_state_acquisition
                                .register_pending_put_tries(put_tries_in_progress);
                        }
                    }
                    (false, true) => {
                        if let EraValidatorsAcquisitionState::PendingGlobalState {
                            global_state_acquisition,
                        } = acquired_parent_block_era_validators.state_mut()
                        {
                            global_state_acquisition
                                .register_pending_put_tries(put_tries_in_progress);
                        }
                    }
                    (true, true) | (false, false) => {}
                }
            }
            BlockAcquisitionState::HaveBlock(_, _, _, None)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => {}
        }
    }

    /// The block height of the current block, if available.
    pub(super) fn block_height(&self) -> Option<u64> {
        match self {
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Failed(..) => None,
            BlockAcquisitionState::HaveBlockHeader {
                header: block_header,
                ..
            }
            | BlockAcquisitionState::HaveWeakFinalitySignatures(block_header, _) => {
                Some(block_header.height())
            }
            BlockAcquisitionState::HaveBlock(block, _, _, _)
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
                if header.block_hash() == *block_hash {
                    info!(
                        "BlockAcquisition: registering header for: {:?}, height: {}",
                        block_hash,
                        header.height()
                    );
                    let state_root_hash = *header.state_root_hash();
                    BlockAcquisitionState::HaveBlockHeader {
                        header: Box::new(header),
                        acquired_signatures: signatures.clone(),
                        acquired_block_era_validators:
                            EraValidatorsAcquisition::new_pending_from_storage(state_root_hash),
                        acquired_parent_block_era_validators: EraValidatorsAcquisition::new(),
                        parent_block_header: None,
                    }
                } else {
                    return Err(BlockAcquisitionError::BlockHashMismatch {
                        expected: *block_hash,
                        actual: header.block_hash(),
                    });
                }
            }
            // we never ask for a block_header while in the following states,
            // and thus it is erroneous to attempt to apply one
            BlockAcquisitionState::HaveBlockHeader { .. }
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

                let global_state_acq = need_execution_state
                    .then(|| GlobalStateAcquisition::new(block.state_root_hash()));
                BlockAcquisitionState::HaveBlock(
                    Box::new(block.clone()),
                    signatures.clone(),
                    deploy_acquisition,
                    global_state_acq,
                )
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader { .. }
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

    /// Register a finality signature as pending for this block.
    pub(super) fn register_finality_signature_pending(&mut self, validator: PublicKey) {
        match self {
            BlockAcquisitionState::HaveBlockHeader {
                acquired_signatures,
                ..
            }
            | BlockAcquisitionState::HaveBlock(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveGlobalState(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveApprovalsHashes(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(_, acquired_signatures, ..)
            | BlockAcquisitionState::HaveAllDeploys(_, acquired_signatures)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, acquired_signatures)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, acquired_signatures) => {
                acquired_signatures.register_pending(validator);
            }
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Failed(..) => {}
        };
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
        let signer = signature.public_key.clone();
        let acceptance: Acceptance;
        let maybe_block_hash: Option<BlockHash>;
        let currently_acquiring_sigs: bool;
        let maybe_new_state: Option<BlockAcquisitionState> = match self {
            BlockAcquisitionState::HaveBlockHeader {
                header,
                acquired_signatures,
                ..
            } => {
                // we are attempting to acquire at least ~1/3 signature weight before
                // committing to doing non-trivial work to acquire this block
                // thus the primary thing we are doing in this state is accumulating sigs.
                // We also want to ensure we've tried at least once to fetch every potential
                // signature.
                maybe_block_hash = Some(header.block_hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                currently_acquiring_sigs = true;
                match acquired_signatures.signature_weight() {
                    SignatureWeight::Insufficient => None,
                    SignatureWeight::Weak | SignatureWeight::Strict => {
                        if acquired_signatures.have_no_vacant() {
                            Some(BlockAcquisitionState::HaveWeakFinalitySignatures(
                                header.clone(),
                                acquired_signatures.clone(),
                            ))
                        } else {
                            None
                        }
                    }
                }
            }
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, acquired_deploys, _) => {
                maybe_block_hash = Some(*block.hash());
                currently_acquiring_sigs = !is_historical
                    && acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict;
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                None
            }
            BlockAcquisitionState::HaveGlobalState(
                block,
                acquired_signatures,
                acquired_deploys,
                ..,
            )
            | BlockAcquisitionState::HaveApprovalsHashes(
                block,
                acquired_signatures,
                acquired_deploys,
            ) => {
                maybe_block_hash = Some(*block.hash());
                currently_acquiring_sigs = acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict;
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
                currently_acquiring_sigs = !acquired_signatures.is_checkable()
                    && acquired_deploys.needs_deploy().is_none()
                    && acquired_signatures.signature_weight() != SignatureWeight::Strict;
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                None
            }
            BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures) => {
                maybe_block_hash = Some(*block.hash());
                currently_acquiring_sigs =
                    acquired_signatures.signature_weight() != SignatureWeight::Strict;
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                None
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(block, acquired_signatures) => {
                maybe_block_hash = Some(*block.hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                currently_acquiring_sigs = false;
                None
            }
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, acquired_signatures) => {
                // 1: In HaveWeakFinalitySignatures we are waiting to acquire the block body
                // 2: In HaveStrictFinalitySignatures we are in the happy path resting state
                // and have enough signatures, but not necessarily all signatures and
                // will accept late comers while resting in this state
                maybe_block_hash = Some(header.block_hash());
                acceptance = acquired_signatures.apply_signature(signature, validator_weights);
                currently_acquiring_sigs = false;
                None
            }
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Failed(..) => {
                return Ok(None)
            }
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
            BlockAcquisitionState::HaveBlock(block, signatures, acquired, _)
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
            | BlockAcquisitionState::HaveBlockHeader { .. }
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

    pub(super) fn register_trie_or_chunk(
        &mut self,
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_or_chunk: TrieOrChunk,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match self {
            BlockAcquisitionState::HaveBlock(_, _, _, Some(global_state_acq)) => {
                match global_state_acq.register_trie_or_chunk(trie_hash, trie_or_chunk) {
                    Ok(()) => Ok(Some(Acceptance::NeededIt)),
                    Err(GlobalStateAcquisitionError::UnexpectedTrieOrChunkRegister(_)) => Ok(None),
                    Err(e) => Err(BlockAcquisitionError::GlobalStateAcquisition(e)),
                }
            }
            BlockAcquisitionState::HaveBlockHeader {
                acquired_block_era_validators,
                acquired_parent_block_era_validators,
                ..
            } => {
                match (
                    acquired_block_era_validators.is_acquiring_from_root_hash(&state_root_hash),
                    acquired_parent_block_era_validators
                        .is_acquiring_from_root_hash(&state_root_hash),
                ) {
                    (true, true) => Err(BlockAcquisitionError::DuplicateGlobalStateAcquisition(
                        state_root_hash,
                    )),
                    (true, false) => Self::update_era_validators_acquisition_with_trie_or_chunk(
                        acquired_block_era_validators,
                        state_root_hash,
                        trie_hash,
                        trie_or_chunk,
                    ),
                    (false, true) => Self::update_era_validators_acquisition_with_trie_or_chunk(
                        acquired_parent_block_era_validators,
                        state_root_hash,
                        trie_hash,
                        trie_or_chunk,
                    ),
                    (false, false) => Ok(None),
                }
            }
            BlockAcquisitionState::HaveBlock(_, _, _, None)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => Ok(None),
        }
    }

    pub(super) fn register_trie_or_chunk_fetch_error(
        &mut self,
        state_root_hash: Digest,
        trie_hash: Digest,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match self {
            BlockAcquisitionState::HaveBlock(_, _, _, Some(global_state_acq)) => {
                match global_state_acq.register_trie_or_chunk_fetch_error(trie_hash) {
                    Ok(()) => Ok(Some(Acceptance::NeededIt)),
                    Err(GlobalStateAcquisitionError::UnexpectedTrieOrChunkRegister(_)) => Ok(None),
                    Err(e) => Err(BlockAcquisitionError::GlobalStateAcquisition(e)),
                }
            }
            BlockAcquisitionState::HaveBlockHeader {
                acquired_block_era_validators,
                acquired_parent_block_era_validators,
                ..
            } => {
                match (
                    acquired_block_era_validators.is_acquiring_from_root_hash(&state_root_hash),
                    acquired_parent_block_era_validators
                        .is_acquiring_from_root_hash(&state_root_hash),
                ) {
                    (true, true) => Err(BlockAcquisitionError::DuplicateGlobalStateAcquisition(
                        state_root_hash,
                    )),
                    (true, false) => {
                        Self::update_era_validators_acquisition_with_trie_or_chunk_fetch_error(
                            acquired_block_era_validators,
                            state_root_hash,
                            trie_hash,
                        )
                    }
                    (false, true) => {
                        Self::update_era_validators_acquisition_with_trie_or_chunk_fetch_error(
                            acquired_parent_block_era_validators,
                            state_root_hash,
                            trie_hash,
                        )
                    }
                    (false, false) => Ok(None),
                }
            }
            BlockAcquisitionState::HaveBlock(_, _, _, None)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => Ok(None),
        }
    }

    pub(super) fn register_put_trie(
        &mut self,
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_raw: TrieRaw,
        put_trie_result: Result<Digest, engine_state::Error>,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        let (maybe_new_state, acceptance) = match self {
            BlockAcquisitionState::HaveBlock(
                block,
                acquired_signatures,
                deploy_acquisition,
                Some(global_state_acq),
            ) => {
                let acceptance = match global_state_acq.register_put_trie(
                    trie_hash,
                    trie_raw,
                    put_trie_result,
                ) {
                    Ok(()) => Ok(Some(Acceptance::NeededIt)),
                    Err(GlobalStateAcquisitionError::UnexpectedTrieOrChunkRegister(_)) => Ok(None),
                    Err(e) => Err(BlockAcquisitionError::GlobalStateAcquisition(e)),
                };

                let maybe_new_state = global_state_acq.is_finished().then(|| {
                    BlockAcquisitionState::HaveGlobalState(
                        block.clone(),
                        acquired_signatures.clone(),
                        deploy_acquisition.clone(),
                        ExecutionResultsAcquisition::Needed {
                            block_hash: *block.hash(),
                        },
                    )
                });

                (maybe_new_state, acceptance)
            }
            BlockAcquisitionState::HaveBlockHeader {
                acquired_block_era_validators,
                acquired_parent_block_era_validators,
                ..
            } => {
                let acceptance = match (
                    acquired_block_era_validators.is_acquiring_from_root_hash(&state_root_hash),
                    acquired_parent_block_era_validators
                        .is_acquiring_from_root_hash(&state_root_hash),
                ) {
                    (true, true) => Err(BlockAcquisitionError::DuplicateGlobalStateAcquisition(
                        state_root_hash,
                    )),
                    (true, false) => Self::update_era_validators_acquisition_with_put_trie_result(
                        acquired_block_era_validators,
                        state_root_hash,
                        trie_hash,
                        trie_raw,
                        put_trie_result,
                    ),
                    (false, true) => Self::update_era_validators_acquisition_with_put_trie_result(
                        acquired_parent_block_era_validators,
                        state_root_hash,
                        trie_hash,
                        trie_raw,
                        put_trie_result,
                    ),
                    (false, false) => Ok(None),
                };
                (None, acceptance)
            }
            BlockAcquisitionState::HaveBlock(_, _, _, None)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => (None, Ok(None)),
        };

        if let Some(new_state) = maybe_new_state {
            self.set_state(new_state);
        }

        acceptance
    }

    pub(super) fn register_block_header_requested_from_storage(
        &mut self,
        block_header: Option<BlockHeader>,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match self {
            BlockAcquisitionState::HaveBlockHeader {
                parent_block_header,
                acquired_parent_block_era_validators,
                ..
            } => {
                if let Some(header) = block_header {
                    if parent_block_header.is_some() {
                        Ok(Some(Acceptance::HadIt))
                    } else {
                        let state_root_hash = *header.state_root_hash();
                        *parent_block_header = Some(header);
                        *acquired_parent_block_era_validators =
                            EraValidatorsAcquisition::new_pending_from_storage(state_root_hash);
                        Ok(Some(Acceptance::NeededIt))
                    }
                } else {
                    Err(BlockAcquisitionError::BlockHeaderMissing)
                }
            }
            BlockAcquisitionState::HaveBlock(_, _, _, _)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => Ok(None),
        }
    }

    fn update_era_validators_acquisition_with_trie_or_chunk(
        era_validators_acquisition: &mut EraValidatorsAcquisition,
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_or_chunk: TrieOrChunk,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match era_validators_acquisition.register_global_state_trie_or_chunk(
            state_root_hash,
            trie_hash,
            trie_or_chunk,
        ) {
            Ok(()) => Ok(Some(Acceptance::NeededIt)),
            Err(EraValidatorsAcquisitionError::AlreadyComplete) => Ok(Some(Acceptance::HadIt)),
            Err(EraValidatorsAcquisitionError::NotAcquiring { .. })
            | Err(EraValidatorsAcquisitionError::GlobalStateAcquisition {
                err: GlobalStateAcquisitionError::UnexpectedTrieOrChunkRegister(_),
            }) => Ok(None),
            Err(e) => Err(BlockAcquisitionError::EraValidatorsAcquisition(e)),
        }
    }

    fn update_era_validators_acquisition_with_trie_or_chunk_fetch_error(
        era_validators_acquisition: &mut EraValidatorsAcquisition,
        state_root_hash: Digest,
        trie_hash: Digest,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match era_validators_acquisition
            .register_global_state_trie_or_chunk_fetch_error(state_root_hash, trie_hash)
        {
            Ok(()) => Ok(Some(Acceptance::NeededIt)),
            Err(EraValidatorsAcquisitionError::AlreadyComplete) => Ok(Some(Acceptance::HadIt)),
            Err(EraValidatorsAcquisitionError::NotAcquiring { .. })
            | Err(EraValidatorsAcquisitionError::GlobalStateAcquisition {
                err: GlobalStateAcquisitionError::UnexpectedTrieOrChunkRegister(_),
            }) => Ok(None),
            Err(e) => Err(BlockAcquisitionError::EraValidatorsAcquisition(e)),
        }
    }

    fn update_era_validators_acquisition_with_put_trie_result(
        era_validators_acquisition: &mut EraValidatorsAcquisition,
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_raw: TrieRaw,
        put_trie_result: Result<Digest, engine_state::Error>,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match era_validators_acquisition.register_global_state_put_trie_result(
            state_root_hash,
            trie_hash,
            trie_raw,
            put_trie_result,
        ) {
            Ok(()) => Ok(Some(Acceptance::NeededIt)),
            Err(EraValidatorsAcquisitionError::AlreadyComplete) => Ok(Some(Acceptance::HadIt)),
            Err(EraValidatorsAcquisitionError::NotAcquiring { .. })
            | Err(EraValidatorsAcquisitionError::GlobalStateAcquisition {
                err: GlobalStateAcquisitionError::UnexpectedTrieOrChunkRegister(_),
            }) => Ok(None),
            Err(e) => Err(BlockAcquisitionError::EraValidatorsAcquisition(e)),
        }
    }

    fn update_era_validators_for_acquisition(
        era_validators_acquisition: &mut EraValidatorsAcquisition,
        state_hash: &Digest,
        era_validators_get_result: Result<EraValidators, EraValidatorsGetError>,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match era_validators_get_result {
            Ok(era_validators) => {
                match era_validators_acquisition.register_era_validators(state_hash, era_validators)
                {
                    Ok(()) => Ok(Some(Acceptance::NeededIt)),
                    Err(EraValidatorsAcquisitionError::AlreadyComplete) => {
                        Ok(Some(Acceptance::HadIt))
                    }
                    Err(EraValidatorsAcquisitionError::NotAcquiring { .. }) => Ok(None),
                    Err(e) => Err(BlockAcquisitionError::EraValidatorsAcquisition(e)),
                }
            }
            Err(EraValidatorsGetError::RootNotFound) => {
                *era_validators_acquisition = EraValidatorsAcquisition::new_pending_global_state(
                    GlobalStateAcquisition::new(state_hash),
                );
                Ok(Some(Acceptance::NeededIt))
            }
            Err(_) => Err(BlockAcquisitionError::MissingEraValidatorWeights),
        }
    }

    pub(super) fn register_era_validators_from_contract_runtime(
        &mut self,
        from_state_root_hash: Digest,
        era_validators_get_result: Result<EraValidators, EraValidatorsGetError>,
    ) -> Result<Option<Acceptance>, BlockAcquisitionError> {
        match self {
            BlockAcquisitionState::HaveBlockHeader {
                acquired_block_era_validators,
                acquired_parent_block_era_validators,
                ..
            } => {
                match (
                    acquired_block_era_validators
                        .is_acquiring_from_root_hash(&from_state_root_hash),
                    acquired_parent_block_era_validators
                        .is_acquiring_from_root_hash(&from_state_root_hash),
                ) {
                    (true, true) => Err(BlockAcquisitionError::DuplicateGlobalStateAcquisition(
                        from_state_root_hash,
                    )),
                    (true, false) => Self::update_era_validators_for_acquisition(
                        acquired_block_era_validators,
                        &from_state_root_hash,
                        era_validators_get_result,
                    ),
                    (false, true) => Self::update_era_validators_for_acquisition(
                        acquired_parent_block_era_validators,
                        &from_state_root_hash,
                        era_validators_get_result,
                    ),
                    (false, false) => Ok(None),
                }
            }
            BlockAcquisitionState::HaveBlock(_, _, _, _)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::Failed(_, _) => Ok(None),
        }
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
            | BlockAcquisitionState::HaveBlockHeader { .. }
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
        debug!(state=%self, need_execution_state,
            block_execution_results_or_chunk=%block_execution_results_or_chunk,
            "register_execution_results_or_chunk");
        let (new_state, ret) = match self {
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
                    Ok(new_effects) => match new_effects {
                        ExecutionResultsAcquisition::Needed { .. }
                        | ExecutionResultsAcquisition::Pending { .. } => {
                            debug!("apply_block_execution_results_or_chunk: Needed | Pending");
                            return Ok(None);
                        }
                        ExecutionResultsAcquisition::Complete { ref results, .. } => {
                            debug!("apply_block_execution_results_or_chunk: Complete");
                            (
                                BlockAcquisitionState::HaveGlobalState(
                                    block.clone(),
                                    signatures.clone(),
                                    deploys.clone(),
                                    new_effects.clone(),
                                ),
                                Some(results.clone()),
                            )
                        }
                        ExecutionResultsAcquisition::Acquiring { .. } => {
                            debug!("apply_block_execution_results_or_chunk: Acquiring");
                            (
                                BlockAcquisitionState::HaveGlobalState(
                                    block.clone(),
                                    signatures.clone(),
                                    deploys.clone(),
                                    new_effects,
                                ),
                                None,
                            )
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
            | BlockAcquisitionState::HaveBlockHeader { .. }
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
            | BlockAcquisitionState::HaveBlockHeader { .. }
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
            BlockAcquisitionState::HaveBlock(block, signatures, deploys, _)
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
            | BlockAcquisitionState::HaveBlockHeader { .. }
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveBlock(_, _, _, _)
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
        info!("BlockAcquisition: registering deploy for: {}", block.hash());
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
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, deploy_acquisition, _)
                if !need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering block execution for: {}",
                    block.hash()
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
                    block.hash()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
                    acquired_signatures.clone(),
                )
            }

            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader { .. }
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
            BlockAcquisitionState::HaveBlock(block, acquired_signatures, deploy_acquisition, _)
                if !need_execution_state =>
            {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.hash()
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
                    block.hash()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
                    acquired_signatures.clone(),
                )
            }
            BlockAcquisitionState::HaveAllDeploys(block, acquired_signatures) => {
                info!(
                    "BlockAcquisition: registering marked complete for: {}",
                    block.hash()
                );
                BlockAcquisitionState::HaveStrictFinalitySignatures(
                    block.clone(),
                    acquired_signatures.clone(),
                )
            }

            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlockHeader { .. }
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

pub(super) fn signatures_from_missing_validators(
    validator_weights: &EraValidatorWeights,
    signatures: &mut SignatureAcquisition,
    max_simultaneous_peers: usize,
    peer_list: &PeerList,
    rng: &mut NodeRng,
    block_header: &BlockHeader,
) -> BlockAcquisitionAction {
    let mut missing_signatures_in_random_order: HashSet<PublicKey> = validator_weights
        .missing_validators(signatures.not_vacant())
        .cloned()
        .collect();
    // If there are too few, retry any in Pending state.
    if missing_signatures_in_random_order.len() < max_simultaneous_peers {
        missing_signatures_in_random_order.extend(
            validator_weights
                .missing_validators(signatures.not_pending())
                .cloned(),
        );
    }
    BlockAcquisitionAction::finality_signatures(
        peer_list,
        rng,
        block_header,
        missing_signatures_in_random_order.into_iter().collect(),
    )
}
