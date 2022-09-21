use itertools::Itertools;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::{mem, rc::Rc};

use datasize::DataSize;
use rand::{prelude::SliceRandom, seq::IteratorRandom, Rng};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, Timestamp};

use crate::components::block_synchronizer::signature_acquisition::SignatureAcquisition;
use crate::types::{BlockHash, Item, SignatureWeight, ValidatorMatrix};
use crate::{
    components::block_synchronizer::{
        deploy_acquisition::DeployAcquisition, deploy_acquisition::DeployState,
        need_next::NeedNext, peer_list::PeerList,
    },
    types::{Block, BlockHeader, DeployHash, FinalitySignature, NodeId},
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
                    "attempt to apply invalid global hash root hash: {}",
                    root_hash
                )
            }
            Error::InvalidAttemptToApplyDeploy { deploy_hash } => {
                write!(f, "attempt to apply invalid deploy {}", deploy_hash)
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

// #[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug, Default)]
// pub(crate) enum GlobalStateStatus {
//     #[default]
//     All,
//     None,
//     Acquiring(Timestamp),
// }

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
enum ExecutionState {
    Unneeded,
    GlobalState(Digest),
    ExecutionEffects(BTreeMap<DeployHash, DeployState>),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum BlockAcquisitionState {
    Initialized(BlockHash, SignatureAcquisition),
    HaveBlockHeader(Box<BlockHeader>, SignatureAcquisition),
    HaveSufficientFinalitySignatures(Box<BlockHeader>, SignatureAcquisition),
    HaveBlock(Box<BlockHeader>, SignatureAcquisition, DeployAcquisition),
    HaveGlobalState(Box<BlockHeader>, SignatureAcquisition, DeployAcquisition),
    HaveDeploys(Box<BlockHeader>, SignatureAcquisition, DeployAcquisition),
    HaveExecutionEffects(Box<BlockHeader>, SignatureAcquisition),
    HaveStrictFinalitySignatures(SignatureAcquisition),
    Fatal,
}

impl BlockAcquisitionState {
    pub(super) fn new(block_hash: BlockHash, validators: Vec<PublicKey>) -> Self {
        BlockAcquisitionState::Initialized(block_hash, SignatureAcquisition::new(validators))
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
            BlockAcquisitionState::HaveBlockHeader(_, _)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_)
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
            BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_)
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidStateTransition),
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_signatures(
        &mut self,
        signatures: Vec<FinalitySignature>,
        validator_matrix: Rc<ValidatorMatrix>,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveBlockHeader(header, acquired) => {
                signatures
                    .into_iter()
                    .map(|fs| acquired.apply_signature(fs));

                match validator_matrix
                    .have_sufficient_weight(header.era_id(), acquired.have_signatures())
                {
                    SignatureWeight::Insufficient => {
                        // Should not change state.
                        return Ok(());
                    }
                    SignatureWeight::Sufficient | SignatureWeight::Strict => {
                        BlockAcquisitionState::HaveSufficientFinalitySignatures(
                            header.clone(),
                            acquired.clone(),
                        )
                    }
                }
            }
            BlockAcquisitionState::HaveDeploys(header, acquired_signatures, deploys)
                if !need_execution_state =>
            {
                signatures
                    .into_iter()
                    .map(|fs| acquired_signatures.apply_signature(fs));

                match validator_matrix
                    .have_sufficient_weight(header.era_id(), acquired_signatures.have_signatures())
                {
                    SignatureWeight::Insufficient | SignatureWeight::Sufficient => {
                        // Should not change state.
                        return Ok(());
                    }
                    SignatureWeight::Strict => BlockAcquisitionState::HaveStrictFinalitySignatures(
                        acquired_signatures.clone(),
                    ),
                }
            }
            BlockAcquisitionState::HaveExecutionEffects(header, acquired)
                if need_execution_state =>
            {
                signatures
                    .into_iter()
                    .map(|fs| acquired.apply_signature(fs));

                match validator_matrix
                    .have_sufficient_weight(header.era_id(), acquired.have_signatures())
                {
                    SignatureWeight::Insufficient | SignatureWeight::Sufficient => {
                        // Should not change state.
                        return Ok(());
                    }
                    SignatureWeight::Strict => {
                        BlockAcquisitionState::HaveStrictFinalitySignatures(acquired.clone())
                    }
                }
            }

            // we never ask for finality signatures while in these states, thus it's always
            // erroneous to attempt to apply any
            BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_)
            | BlockAcquisitionState::Fatal => return Err(Error::InvalidAttemptToApplySignatures),
        };
        *self = new_state;
        Ok(())
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
                    None => BlockAcquisitionState::HaveDeploys(
                        header.clone(),
                        signatures.clone(),
                        acquired.clone(),
                    ),
                    Some(_) => {
                        // Should not change state.
                        return Ok(());
                    }
                }
            }
            BlockAcquisitionState::HaveGlobalState(header, signatures, acquired)
                if need_execution_state =>
            {
                acquired.apply_deploy(deploy_hash);
                match acquired.needs_deploy() {
                    None => BlockAcquisitionState::HaveDeploys(
                        header.clone(),
                        signatures.clone(),
                        acquired.clone(),
                    ),
                    Some(_) => {
                        // Should not change state.
                        return Ok(());
                    }
                }
            }
            // we never ask for deploys in the following states, and thus it is erroneous to attempt
            // to apply any
            BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveBlockHeader(_, _)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_)
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
                    )
                } else {
                    return Err(Error::RootHashMismatch {
                        expected: *header.state_root_hash(),
                        actual: root_hash,
                    });
                }
            }
            // we never ask for global state in the following states, and thus it is erroneous to attempt
            // to apply any
            BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveBlockHeader(_, _)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_)
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyGlobalState { root_hash });
            }
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn with_execution_results(
        &mut self,
        deploy_hash: DeployHash,
        need_execution_state: bool,
    ) -> Result<(), Error> {
        let new_state = match self {
            BlockAcquisitionState::HaveDeploys(header, signatures, ref mut acquired)
                if need_execution_state =>
            {
                acquired.apply_execution_effect(deploy_hash);
                match acquired.needs_execution_result() {
                    None => BlockAcquisitionState::HaveExecutionEffects(
                        header.clone(),
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
            BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveBlockHeader(_, _)
            | BlockAcquisitionState::HaveSufficientFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_)
            | BlockAcquisitionState::Fatal => {
                return Err(Error::InvalidAttemptToApplyDeploy { deploy_hash });
            }
        };
        *self = new_state;
        Ok(())
    }

    pub(super) fn next_action(
        &mut self,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        should_fetch_execution_state: bool,
        validator_matrix: Rc<ValidatorMatrix>,
    ) -> Result<BlockAcquisitionAction, Error> {
        match self {
            BlockAcquisitionState::Initialized(block_hash, signatures) => Ok(
                BlockAcquisitionAction::block_header(peer_list, rng, *block_hash),
            ),
            BlockAcquisitionState::HaveBlockHeader(header, signatures) => {
                let era_id = header.era_id();
                let block_header = header.as_ref();
                match validator_matrix.validator_public_keys(era_id) {
                    None => Ok(BlockAcquisitionAction::era_validators(era_id)),
                    Some(validators) => Ok(BlockAcquisitionAction::finality_signatures(
                        peer_list,
                        rng,
                        block_header,
                        validators,
                    )),
                }
            }
            BlockAcquisitionState::HaveSufficientFinalitySignatures(header, _) => Ok(
                BlockAcquisitionAction::block_body(peer_list, rng, header.id()),
            ),
            BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _)
            | BlockAcquisitionState::HaveDeploys(_, _, _)
            | BlockAcquisitionState::HaveExecutionEffects(_, _) => self
                .resolve_execution_state_divergence(
                    peer_list,
                    rng,
                    should_fetch_execution_state,
                    validator_matrix,
                ),
            BlockAcquisitionState::HaveStrictFinalitySignatures(_)
            | BlockAcquisitionState::Fatal => Ok(BlockAcquisitionAction::noop()),
        }
    }

    fn resolve_execution_state_divergence(
        &self,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        should_fetch_execution_state: bool,
        validator_matrix: Rc<ValidatorMatrix>,
    ) -> Result<BlockAcquisitionAction, Error> {
        // squash complexity of need all / don't need all and other
        // irritating hoop jumping around getting
        // deploys, global state, and execution effects
        enum Mode {
            GetGlobalState(Box<BlockHeader>, Digest),
            GetDeploy(Box<BlockHeader>, DeployHash),
            GetExecResult(Box<BlockHeader>),
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
            (BlockAcquisitionState::HaveGlobalState(header, signatures, deploy_state), true) => {
                match deploy_state.needs_deploy() {
                    Some(deploy_hash) => Mode::GetDeploy(header, deploy_hash),
                    None => match deploy_state.needs_execution_result() {
                        Some(_deploy_hash) => Mode::GetExecResult(header),
                        None => Mode::GetStrictSignatures(header, signatures),
                    },
                }
            }
            (BlockAcquisitionState::HaveGlobalState(header, signatures, deploy_state), false) => {
                match deploy_state.needs_deploy() {
                    Some(deploy_hash) => Mode::GetDeploy(header, deploy_hash),
                    None => Mode::GetStrictSignatures(header, signatures),
                }
            }
            (BlockAcquisitionState::HaveDeploys(header, signatures, deploy_state), true) => {
                match deploy_state.needs_execution_result() {
                    Some(_deploy_hash) => Mode::GetExecResult(header),
                    None => Mode::GetStrictSignatures(header, signatures),
                }
            }
            (BlockAcquisitionState::HaveDeploys(header, signatures, _), false) => {
                Mode::GetStrictSignatures(header, signatures)
            }
            _ => Mode::Err, // this must be programmer error
        };

        match mode {
            Mode::GetGlobalState(block_header, root_hash) => Ok(
                BlockAcquisitionAction::global_state(peer_list, rng, root_hash),
            ),
            Mode::GetDeploy(block_header, deploy_hash) => {
                Ok(BlockAcquisitionAction::deploy(peer_list, rng, deploy_hash))
            }
            Mode::GetExecResult(block_header) => Ok(BlockAcquisitionAction::execution_results(
                peer_list,
                rng,
                block_header.id(),
            )),
            Mode::GetStrictSignatures(block_header, acquired) => {
                let header = block_header.as_ref();
                let era_id = header.era_id();

                match validator_matrix.missing_signatures(era_id, &acquired.have_signatures()) {
                    Ok(missing_signatures) => Ok(BlockAcquisitionAction::finality_signatures(
                        peer_list,
                        rng,
                        header,
                        missing_signatures,
                    )),
                    Err(_) => Ok(BlockAcquisitionAction::era_validators(era_id)),
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

    pub(super) fn noop() -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Nothing,
        }
    }

    pub(super) fn peers() -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Peers,
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

    pub(super) fn deploy(peer_list: &PeerList, rng: &mut NodeRng, deploy_hash: DeployHash) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::Deploy(deploy_hash),
        }
    }

    pub(super) fn global_state(peer_list: &PeerList, rng: &mut NodeRng, root_hash: Digest) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::GlobalState(root_hash),
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
