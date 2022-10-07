use std::{
    collections::{BTreeMap, HashMap},
    fmt::{Display, Formatter},
};

use datasize::DataSize;
use either::Either;
use tracing::debug;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::{DeployAcquisition, DeployState},
        need_next::NeedNext,
        peer_list::PeerList,
        signature_acquisition::SignatureAcquisition,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId,
        BlockHash, BlockHeader, DeployHash, DeployId, EraValidatorWeights, FinalitySignature, Item,
        NodeId, SignatureWeight,
    },
    NodeRng,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
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
    InvalidAttemptToApplySignatures,
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
    UnexpectedExecutionResultsState,
    ExecutionResults(super::execution_results_acquisition::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidStateTransition => write!(f, "invalid state transition"),
            Error::InvalidAttemptToApplySignatures => {
                write!(f, "invalid attempt to apply signatures")
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
            Error::UnexpectedExecutionResultsState => {
                write!(f, "unexpected execution results state")
            }
            Error::ExecutionResults(error) => write!(f, "execution results error: {}", error),
        }
    }
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
enum ExecutionState {
    Unneeded,
    GlobalState(Digest),
    ExecutionResults(BTreeMap<DeployHash, DeployState>),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum BlockAcquisitionState {
    Initialized(BlockHash, SignatureAcquisition),
    HaveBlockHeader(Box<BlockHeader>, SignatureAcquisition),
    HaveWeakFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    HaveBlock(Box<Block>, SignatureAcquisition, DeployAcquisition),
    HaveGlobalState(
        Box<BlockHeader>,
        SignatureAcquisition,
        DeployAcquisition,
        ExecutionResultsAcquisition,
    ),
    HaveAllExecutionResults(Box<BlockHeader>, SignatureAcquisition, DeployAcquisition),
    HaveAllDeploys(Box<BlockHeader>, SignatureAcquisition),
    HaveStrictFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    Fatal,
}

impl BlockAcquisitionState {
    pub(super) fn new(block_hash: BlockHash, validators: Vec<PublicKey>) -> Self {
        BlockAcquisitionState::Initialized(block_hash, SignatureAcquisition::new(validators))
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        match self {
            BlockAcquisitionState::Initialized(..) | BlockAcquisitionState::Fatal => None,
            BlockAcquisitionState::HaveBlockHeader(header, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(header, _)
            | BlockAcquisitionState::HaveGlobalState(header, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(header, _, _)
            | BlockAcquisitionState::HaveAllDeploys(header, ..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(header, _) => {
                Some(header.height())
            }
            BlockAcquisitionState::HaveBlock(block, _, _) => Some(block.height()),
        }
    }

    pub(super) fn with_header(&mut self, header: BlockHeader) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::Initialized(block_hash, signatures) => {
                if header.id() == *block_hash {
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
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidStateTransition),
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        todo!();
    }

    pub(super) fn with_block(
        &mut self,
        block: &Block,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlockHeader(header, signatures) => {
                let expected_block_hash = header.block_hash();
                let actual_block_hash = block.hash();
                if *actual_block_hash != expected_block_hash {
                    return Err(Error::BlockHashMismatch {
                        expected: expected_block_hash,
                        actual: *actual_block_hash,
                    });
                }

                let deploy_hashes = block.deploy_and_transfer_hashes().copied().collect();
                let deploy_acquisition =
                    DeployAcquisition::new_by_hash(deploy_hashes, need_execution_state);

                BlockAcquisitionState::HaveBlock(
                    Box::new(block.clone()),
                    signatures.clone(),
                    deploy_acquisition,
                )
            }
            // we do not ask for a block's body while in the following states, and
            // thus it is erroneous to attempt to apply one
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidStateTransition),
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_signature(
        &mut self,
        signature: FinalitySignature,
        validator_weights: EraValidatorWeights,
    ) -> Result<bool, Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlockHeader(header, acquired_signatures) => {
                if !acquired_signatures.apply_signature(signature) {
                    return Ok(false);
                };

                match validator_weights.has_sufficient_weight(acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient => {
                        // Should not change state.
                        return Ok(true);
                    }
                    SignatureWeight::Weak | SignatureWeight::Sufficient => {
                        BlockAcquisitionState::HaveWeakFinalitySignatures(
                            header.clone(),
                            acquired_signatures.clone(),
                        )
                    }
                }
            }
            BlockAcquisitionState::HaveAllDeploys(header, acquired_signatures) => {
                if !acquired_signatures.apply_signature(signature) {
                    return Ok(false);
                };

                match validator_weights.has_sufficient_weight(acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient | SignatureWeight::Weak => {
                        // Should not change state.
                        return Ok(true);
                    }
                    SignatureWeight::Sufficient => {
                        BlockAcquisitionState::HaveStrictFinalitySignatures(
                            header.clone(),
                            acquired_signatures.clone(),
                        )
                    }
                }
            }

            // we never ask for finality signatures while in these states, thus it's always
            // erroneous to attempt to apply any
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidAttemptToApplySignatures),
        };
        *self = new_state;
        Ok(true)
    }

    pub(super) fn with_deploy(
        &mut self,
        deploy_id: DeployId,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, acquired)
                if !need_execution_state =>
            {
                acquired.apply_deploy(deploy_id);
                match acquired.needs_deploy() {
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
            BlockAcquisitionState::HaveAllExecutionResults(header, signatures, deploys)
                if need_execution_state =>
            {
                deploys.apply_deploy(deploy_id);
                match deploys.needs_deploy() {
                    None => {
                        BlockAcquisitionState::HaveAllDeploys(header.clone(), signatures.clone())
                    }
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
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyDeploy { deploy_id });
            }
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_global_state(
        &mut self,
        root_hash: Digest,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(block, signatures, deploys)
                if need_execution_state =>
            {
                if block.state_root_hash() == &root_hash {
                    BlockAcquisitionState::HaveGlobalState(
                        Box::new(block.header().clone()),
                        signatures.clone(),
                        deploys.clone(),
                        ExecutionResultsAcquisition::Needed {
                            block_hash: *block.hash(),
                        },
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
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyGlobalState { root_hash });
            }
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_execution_results_root_hash(
        &mut self,
        execution_results_check_sum: ExecutionResultsChecksum,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        match self {
            BlockAcquisitionState::HaveGlobalState(
                _,
                _,
                _,
                acq @ ExecutionResultsAcquisition::Needed { .. },
            ) if need_execution_state => {
                if let Err(error) = acq.clone().apply_check_sum(execution_results_check_sum) {
                    return Err(Error::ExecutionResults(error));
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
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyExecutionResultsChecksum);
            }
        };
        Ok(())
    }

    pub(super) fn with_execution_results_or_chunk(
        &mut self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
        need_execution_state: bool,
    ) -> Result<Option<HashMap<DeployHash, casper_types::ExecutionResult>>, Error> {
        let (new_state, ret) = match self {
            BlockAcquisitionState::HaveGlobalState(
                header,
                signatures,
                deploys,
                exec_results_acq,
            ) if need_execution_state => {
                match exec_results_acq
                    .clone()
                    .apply_block_execution_results_or_chunk(block_execution_results_or_chunk)
                {
                    Ok(new_effects) => match new_effects {
                        ExecutionResultsAcquisition::Unneeded { .. }
                        | ExecutionResultsAcquisition::Needed { .. }
                        | ExecutionResultsAcquisition::Pending { .. }
                        | ExecutionResultsAcquisition::Incomplete { .. }
                        | ExecutionResultsAcquisition::Complete { .. } => {
                            return Ok(None);
                        }
                        ExecutionResultsAcquisition::Mapped { ref results, .. } => (
                            BlockAcquisitionState::HaveGlobalState(
                                header.clone(),
                                signatures.clone(),
                                deploys.clone(),
                                new_effects.clone(),
                            ),
                            Some(results.clone()),
                        ),
                    },
                    Err(error) => return Err(Error::ExecutionResults(error)),
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
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyExecutionResults)
            }
        };
        *self = new_state;
        Ok(ret)
    }

    pub(super) fn with_execution_results_stored_notification(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveGlobalState(
                header,
                signatures,
                deploys,
                ExecutionResultsAcquisition::Mapped { .. },
            ) if need_execution_state => BlockAcquisitionState::HaveAllExecutionResults(
                header.clone(),
                signatures.clone(),
                deploys.clone(),
            ),
            BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyStoredExecutionResults);
            }
        };
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
        match self {
            BlockAcquisitionState::Initialized(block_hash, ..) => Ok(
                BlockAcquisitionAction::block_header(peer_list, rng, *block_hash),
            ),
            BlockAcquisitionState::HaveBlockHeader(block_header, signatures) => {
                if validator_weights.is_empty() {
                    return Ok(BlockAcquisitionAction::era_validators(
                        validator_weights.era_id(),
                    ));
                }
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
            BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) => Ok(
                BlockAcquisitionAction::block_body(peer_list, rng, header.id()),
            ),
            BlockAcquisitionState::HaveBlock(block, signatures, deploy_state) => {
                if should_fetch_execution_state {
                    return Ok(BlockAcquisitionAction::global_state(
                        peer_list,
                        rng,
                        *block.hash(),
                        *block.state_root_hash(),
                    ));
                }
                if let Some(either) = deploy_state.needs_deploy() {
                    return Ok(BlockAcquisitionAction::deploy(
                        *block.hash(),
                        peer_list,
                        rng,
                        either,
                    ));
                }
                // TODO: this seems safe to remove as we do this check in `HaveBlockHeader` and the
                // weights don't seem to mutate.
                if validator_weights.is_empty() {
                    return Ok(BlockAcquisitionAction::era_validators(
                        validator_weights.era_id(),
                    ));
                }
                Ok(BlockAcquisitionAction::finality_signatures(
                    peer_list,
                    rng,
                    block.header(),
                    validator_weights
                        .missing_validators(signatures.have_signatures())
                        .cloned()
                        .collect(),
                ))
            }
            BlockAcquisitionState::HaveGlobalState(header, _, _, exec_results) => {
                if should_fetch_execution_state == false {
                    return Err(Error::InvalidStateTransition);
                }
                match exec_results {
                    ExecutionResultsAcquisition::Unneeded { .. }
                    | ExecutionResultsAcquisition::Complete { .. }
                    | ExecutionResultsAcquisition::Mapped { .. } => {
                        Err(Error::InvalidAttemptToAcquireExecutionResults)
                    }
                    ExecutionResultsAcquisition::Needed { .. } => {
                        Ok(BlockAcquisitionAction::execution_results_root_hash(
                            header.block_hash(),
                            *header.state_root_hash(),
                        ))
                    }
                    acq @ ExecutionResultsAcquisition::Pending { .. }
                    | acq @ ExecutionResultsAcquisition::Incomplete { .. } => {
                        match acq.needs_value_or_chunk() {
                            None => {
                                debug!(block_hash=%header.block_hash(), "by design, execution_results_acquisition.needs_value_or_chunk() should never be None for these variants for block_hash");
                                Err(Error::InvalidAttemptToAcquireExecutionResults)
                            }
                            Some(next) => Ok(BlockAcquisitionAction::execution_results(
                                header.block_hash(),
                                peer_list,
                                rng,
                                next,
                            )),
                        }
                    }
                }
            }
            BlockAcquisitionState::HaveAllExecutionResults(header, signatures, deploys) => {
                if should_fetch_execution_state == false {
                    return Err(Error::InvalidStateTransition);
                }
                match deploys.needs_deploy() {
                    Some(either) => Ok(BlockAcquisitionAction::deploy(
                        header.block_hash(),
                        peer_list,
                        rng,
                        either,
                    )),
                    None => Ok(BlockAcquisitionAction::finality_signatures(
                        peer_list,
                        rng,
                        header.as_ref(),
                        validator_weights
                            .missing_validators(signatures.have_signatures())
                            .cloned()
                            .collect(),
                    )),
                }
            }
            BlockAcquisitionState::HaveAllDeploys(header, signatures) => {
                Ok(BlockAcquisitionAction::finality_signatures(
                    peer_list,
                    rng,
                    header.as_ref(),
                    validator_weights
                        .missing_validators(signatures.have_signatures())
                        .cloned()
                        .collect(),
                ))
            }
            BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => Ok(BlockAcquisitionAction::noop()),
        }
    }
}

pub(crate) struct BlockAcquisitionAction {
    peers_to_ask: Vec<NodeId>,
    need_next: NeedNext,
}

impl BlockAcquisitionAction {
    pub(super) fn new(peers_to_ask: Vec<NodeId>, need_next: NeedNext) -> Self {
        BlockAcquisitionAction {
            peers_to_ask,
            need_next,
        }
    }

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
            need_next: NeedNext::ExecutionResultsRootHash {
                block_hash,
                global_state_root_hash,
            },
        }
    }

    pub(super) fn execution_results(
        block_hash: BlockHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        next: BlockExecutionResultsOrChunkId,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::ExecutionResults(block_hash, next),
        }
    }

    pub(super) fn deploy(
        block_hash: BlockHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        deploy_hash_or_id: Either<DeployHash, DeployId>,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::Deploy(block_hash, deploy_hash_or_id),
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

    pub(super) fn build(self) -> (Vec<NodeId>, NeedNext) {
        (self.peers_to_ask, self.need_next)
    }
}
