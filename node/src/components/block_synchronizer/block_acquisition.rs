use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

use datasize::DataSize;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::{DeployAcquisition, DeployState},
        need_next::NeedNext,
        peer_list::PeerList,
        signature_acquisition::SignatureAcquisition,
        ExecutionResultsAcquisition, ExecutionResultsRootHash,
    },
    types::{
        Block, BlockHash, BlockHeader, DeployHash, EraValidatorWeights, FinalitySignature, Item,
        NodeId, SignatureWeight,
    },
    NodeRng,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    InvalidStateTransition,
    InvalidAttemptToApplySignatures,
    InvalidAttemptToApplyGlobalState {
        root_hash: Digest,
    },
    InvalidAttemptToApplyDeploy {
        deploy_hash: DeployHash,
    },
    InvalidAttemptToApplyExecutionResults,
    InvalidAttemptToApplyExecutionResultsRootHash,
    BlockHashMismatch {
        expected: BlockHash,
        actual: BlockHash,
    },
    RootHashMismatch {
        expected: Digest,
        actual: Digest,
    },
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
            Error::InvalidAttemptToApplyDeploy { deploy_hash } => {
                write!(f, "invalid attempt to apply invalid deploy {}", deploy_hash)
            }
            Error::InvalidAttemptToApplyExecutionResults => {
                write!(f, "invalid attempt to apply execution results")
            }
            Error::InvalidAttemptToApplyExecutionResultsRootHash => {
                write!(f, "invalid attempt to apply execution results root hash")
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
pub(crate) enum BlockAcquisitionState {
    Initialized(BlockHash, SignatureAcquisition),
    HaveBlockHeader(Box<BlockHeader>, SignatureAcquisition),
    HaveSufficientFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    HaveBlock(Box<BlockHeader>, SignatureAcquisition, DeployAcquisition),
    HaveGlobalState(
        Box<BlockHeader>,
        SignatureAcquisition,
        DeployAcquisition,
        Option<ExecutionResultsRootHash>,
    ),
    HaveAllDeploys(
        Box<BlockHeader>,
        SignatureAcquisition,
        Option<ExecutionResultsAcquisition>,
    ),
    HaveAllExecutionResults(Box<BlockHeader>, SignatureAcquisition),
    HaveStrictFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    //Enqueued(Box<BlockHeader>, SignatureAcquisition),
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
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(header, _)
            | BlockAcquisitionState::HaveBlock(header, _, _)
            | BlockAcquisitionState::HaveGlobalState(header, ..)
            | BlockAcquisitionState::HaveAllDeploys(header, ..)
            | BlockAcquisitionState::HaveAllExecutionResults(header, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(header, _) => {
                Some(header.height())
            }
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
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidStateTransition),
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_body(
        &mut self,
        block: &Block,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let block_hash = *block.hash();
        let new_state = match self {
            BlockAcquisitionState::HaveBlockHeader(header, signatures) => {
                if header.id() == block_hash {
                    let deploy_hashes = block
                        .deploy_hashes()
                        .iter()
                        .chain(block.body().transfer_hashes())
                        .copied()
                        .collect();
                    BlockAcquisitionState::HaveBlock(
                        header.clone(),
                        signatures.clone(),
                        DeployAcquisition::new(deploy_hashes, need_execution_state),
                    )
                } else {
                    return Err(Error::BlockHashMismatch {
                        expected: block_hash,
                        actual: header.id(),
                    });
                }
            }
            // we do not ask for a block's body while in the following states, and
            // thus it is erroneous to attempt to apply one
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
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
        should_fetch_execution_state: bool,
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
                        BlockAcquisitionState::HaveSufficientFinalitySignatures(
                            header.clone(),
                            acquired_signatures.clone(),
                        )
                    }
                }
            }
            BlockAcquisitionState::HaveAllDeploys(header, acquired_signatures, None)
                if !should_fetch_execution_state =>
            {
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
            BlockAcquisitionState::HaveAllExecutionResults(header, acquired_signatures)
                if should_fetch_execution_state =>
            {
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
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidAttemptToApplySignatures),
        };
        *self = new_state;
        Ok(true)
    }

    pub(super) fn with_deploy(
        &mut self,
        deploy_hash: DeployHash,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlock(header, signatures, acquired)
                if !need_execution_state =>
            {
                acquired.apply_deploy(deploy_hash);
                match acquired.needs_deploy() {
                    None => BlockAcquisitionState::HaveAllDeploys(
                        header.clone(),
                        signatures.clone(),
                        None,
                    ),
                    Some(_) => {
                        // Should not change state.
                        return Ok(());
                    }
                }
            }
            BlockAcquisitionState::HaveGlobalState(
                header,
                signatures,
                acquired,
                Some(execution_results_root_hash),
            ) if need_execution_state => {
                acquired.apply_deploy(deploy_hash);
                match acquired.needs_deploy() {
                    None => BlockAcquisitionState::HaveAllDeploys(
                        header.clone(),
                        signatures.clone(),
                        Some(ExecutionResultsAcquisition::new(
                            (*header).hash(),
                            *execution_results_root_hash,
                        )),
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
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyDeploy { deploy_hash });
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
            BlockAcquisitionState::HaveBlock(header, signatures, deploys)
                if need_execution_state =>
            {
                if header.state_root_hash() == &root_hash {
                    BlockAcquisitionState::HaveGlobalState(
                        header.clone(),
                        signatures.clone(),
                        deploys.clone(),
                        None,
                    )
                } else {
                    return Err(Error::RootHashMismatch {
                        expected: *header.state_root_hash(),
                        actual: root_hash,
                    });
                }
            }
            // we never ask for global state in the following states, and thus it is erroneous to
            // attempt to apply any
            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
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
        execution_results_root_hash: ExecutionResultsRootHash,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        match self {
            BlockAcquisitionState::HaveGlobalState(_, _, _, maybe_execution_results_root_hash)
                if need_execution_state =>
            {
                *maybe_execution_results_root_hash = Some(execution_results_root_hash);
            }
            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyExecutionResultsRootHash);
            }
        };
        Ok(())
    }

    pub(super) fn with_execution_results(
        &mut self,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveAllDeploys(
                header,
                signatures,
                Some(_execution_results_acquisition),
            ) if need_execution_state => {
                BlockAcquisitionState::HaveAllExecutionResults(header.clone(), signatures.clone())
            }
            // we never ask for deploys in the following states, and thus it is erroneous to attempt
            // to apply any
            BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyExecutionResults);
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
            BlockAcquisitionState::Initialized(block_hash, signatures) => Ok(
                BlockAcquisitionAction::block_header(peer_list, rng, *block_hash),
            ),
            BlockAcquisitionState::HaveBlockHeader(block_header, acquired) => {
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
                            .missing_validators(acquired.have_signatures())
                            .cloned()
                            .collect(),
                    ))
                }
            }
            BlockAcquisitionState::HaveSufficientFinalitySignatures(header, _) => Ok(
                BlockAcquisitionAction::block_body(peer_list, rng, header.id()),
            ),
            BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::Fatal => Ok(BlockAcquisitionAction::noop()),

            BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..) => self
                .resolve_execution_state_divergence(
                    peer_list,
                    validator_weights,
                    rng,
                    should_fetch_execution_state,
                ),
        }
    }

    fn resolve_execution_state_divergence(
        &self,
        peer_list: &PeerList,
        validator_weights: &EraValidatorWeights,
        rng: &mut NodeRng,
        should_fetch_execution_state: bool,
    ) -> Result<BlockAcquisitionAction, Error> {
        enum Mode {
            GetGlobalState(Box<BlockHeader>, Digest),
            GetExecResultRootHash(Box<BlockHeader>),
            GetDeploy(Box<BlockHeader>, DeployHash),
            GetExecResult(Box<BlockHeader>, ExecutionResultsAcquisition),
            GetStrictSignatures(Box<BlockHeader>, SignatureAcquisition),
            Err,
        }
        let mode = match (self.clone(), should_fetch_execution_state) {
            (BlockAcquisitionState::HaveBlock(header, signatures, deploy_state), true) => {
                let state_root_hash = *header.state_root_hash();
                Mode::GetGlobalState(header, state_root_hash)
            }
            (BlockAcquisitionState::HaveBlock(header, signatures, deploy_state), false) => {
                match deploy_state.needs_deploy() {
                    Some(deploy_hash) => Mode::GetDeploy(header, deploy_hash),
                    None => Mode::GetStrictSignatures(header, signatures),
                }
            }
            (BlockAcquisitionState::HaveGlobalState(header, _, _, None), true) => {
                Mode::GetExecResultRootHash(header)
            }
            (
                BlockAcquisitionState::HaveGlobalState(
                    header,
                    _signatures,
                    deploy_state,
                    Some(execution_results_root_hash),
                ),
                true,
            ) => match deploy_state.needs_deploy() {
                Some(deploy_hash) => Mode::GetDeploy(header, deploy_hash),
                None => {
                    let block_hash = (*header).hash();
                    Mode::GetExecResult(
                        header,
                        ExecutionResultsAcquisition::new(block_hash, execution_results_root_hash),
                    )
                }
            },
            (
                BlockAcquisitionState::HaveAllDeploys(
                    header,
                    _signatures,
                    Some(execution_results_acquisition),
                ),
                true,
            ) => Mode::GetExecResult(header, execution_results_acquisition),
            (BlockAcquisitionState::HaveAllDeploys(header, signatures, None), false) => {
                Mode::GetStrictSignatures(header, signatures)
            }
            _ => Mode::Err, // this must be programmer error
        };

        match mode {
            Mode::GetGlobalState(block_header, root_hash) => {
                Ok(BlockAcquisitionAction::global_state(
                    peer_list,
                    rng,
                    (*block_header).hash(),
                    root_hash,
                ))
            }
            Mode::GetExecResultRootHash(block_header) => {
                Ok(BlockAcquisitionAction::execution_results_root_hash(
                    *block_header.state_root_hash(),
                ))
            }
            Mode::GetDeploy(block_header, deploy_hash) => Ok(BlockAcquisitionAction::deploy(
                (*block_header).hash(),
                peer_list,
                rng,
                deploy_hash,
            )),
            Mode::GetExecResult(block_header, execution_results_acquisition) => Ok(
                BlockAcquisitionAction::execution_results(peer_list, rng, block_header.id()),
            ),
            Mode::GetStrictSignatures(block_header, acquired) => {
                if validator_weights.is_empty() {
                    Ok(BlockAcquisitionAction::era_validators(
                        validator_weights.era_id(),
                    ))
                } else {
                    Ok(BlockAcquisitionAction::finality_signatures(
                        peer_list,
                        rng,
                        block_header.as_ref(),
                        validator_weights
                            .missing_validators(acquired.have_signatures())
                            .cloned()
                            .collect(),
                    ))
                }
            }
            Mode::Err => Err(Error::InvalidStateTransition),
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

    pub(super) fn execution_results(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::ExecutionResults(block_hash),
        }
    }

    pub(super) fn execution_results_root_hash(global_state_root_hash: Digest) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::ExecutionResultsRootHash {
                global_state_root_hash,
            },
        }
    }

    pub(super) fn deploy(
        block_hash: BlockHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        deploy_hash: DeployHash,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::Deploy(block_hash, deploy_hash),
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
        let block_hash = block_header.hash();
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
