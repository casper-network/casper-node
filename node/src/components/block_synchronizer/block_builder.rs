use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashSet};
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use openssl::pkey::Public;
use rand::{prelude::SliceRandom, seq::IteratorRandom, Rng};
use tracing::error;

use crate::components::block_synchronizer::block_acquisition;
use crate::components::block_synchronizer::block_acquisition::{
    BlockAcquisitionAction, BlockAcquisitionState,
};
use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, Timestamp, U512};

use crate::components::block_synchronizer::{
    deploy_acquisition::DeployAcquisition, need_next::NeedNext, peer_list::PeerList,
    signature_acquisition::SignatureAcquisition,
};
use crate::types::{
    Block, BlockAdded, BlockBody, BlockHash, BlockHeader, DeployHash, FinalitySignature, NodeId,
    SignatureWeight, ValidatorMatrix,
};
use crate::utils::Source::Peer;
use crate::NodeRng;

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    BlockAcquisitionError(block_acquisition::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BlockAcquisitionError(err) => write!(f, "block acquisition error: {}", err),
        }
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockBuilder {
    // imputed
    peer_list: PeerList,
    validator_matrix: Rc<ValidatorMatrix>,

    //fault_tolerance_fraction: Ratio<u64>,
    should_fetch_execution_state: bool,
    block_hash: BlockHash,

    // progress tracking
    started: Option<Timestamp>,
    last_progress_time: Option<Timestamp>,

    // acquired state
    acquisition_state: BlockAcquisitionState,
}

impl BlockBuilder {
    pub(crate) fn new(
        block_hash: BlockHash,
        era_id: EraId,
        validator_matrix: Rc<ValidatorMatrix>,
        should_fetch_execution_state: bool,
    ) -> Self {
        let simultaneous_peers = 3;
        let public_keys = validator_matrix.validator_public_keys(era_id);
        BlockBuilder {
            block_hash,
            validator_matrix,
            acquisition_state: BlockAcquisitionState::Initialized(
                block_hash,
                SignatureAcquisition::new(todo!("public_keys")),
            ),
            peer_list: PeerList::new(simultaneous_peers),
            should_fetch_execution_state,
            started: None,
            last_progress_time: None,
        }
    }

    pub(crate) fn abort(&mut self) -> bool {
        self.acquisition_state = BlockAcquisitionState::Fatal;
        self.flush();
        self.touch();
        self.is_fatal()
    }

    pub(crate) fn builder_state(&self) -> &BlockAcquisitionState {
        &self.acquisition_state
    }

    pub(crate) fn started(&self) -> Option<Timestamp> {
        self.started
    }

    pub(crate) fn last_progress_time(&self) -> Option<Timestamp> {
        self.last_progress_time
    }

    pub(crate) fn is_fatal(&self) -> bool {
        self.acquisition_state == BlockAcquisitionState::Fatal
    }

    pub(crate) fn is_complete(&self) -> bool {
        matches!(
            self.acquisition_state,
            BlockAcquisitionState::HaveStrictFinalitySignatures(_)
        )
    }

    pub(crate) fn register_peer(&mut self, peer: NodeId) {
        if self.is_complete() || self.is_fatal() {
            return;
        }
        self.peer_list.register_peer(peer);
    }

    pub(crate) fn dishonest_peers(&self) -> Vec<NodeId> {
        self.peer_list.dishonest_peers()
    }

    pub(crate) fn disqualify_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.disqualify_peer(peer);
    }

    pub(crate) fn promote_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.promote_peer(peer);
    }

    pub(crate) fn demote_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.demote_peer(peer);
    }

    pub(crate) fn flush_dishonest_peers(&mut self) {
        self.peer_list.flush_dishonest_peers();
    }

    pub(crate) fn flush(&mut self) {
        self.peer_list.flush();
    }

    pub(crate) fn touch(&mut self) {
        let now = Timestamp::now();
        if self.started.is_none() {
            self.started = Some(now);
        }
        self.last_progress_time = Some(now);
    }

    pub(crate) fn next_needed(&mut self, rng: &mut NodeRng) -> BlockAcquisitionAction {
        if self.peer_list.need_peers() {
            return BlockAcquisitionAction::peers();
        }
        match self.acquisition_state.next_action(
            &self.peer_list,
            rng,
            self.should_fetch_execution_state,
            self.validator_matrix.clone(),
        ) {
            Ok(ret) => ret,
            Err(err) => {
                self.acquisition_state = BlockAcquisitionState::Fatal;
                error!(%err);
                BlockAcquisitionAction::noop()
            }
        }
    }

    pub(crate) fn apply_header(
        &mut self,
        block_header: BlockHeader,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self.acquisition_state.with_header(block_header) {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisitionError(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_block(
        &mut self,
        block: &Block,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .with_body(&block, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisitionError(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        self.apply_finality_signatures(vec![finality_signature], maybe_peer)
    }

    pub(crate) fn apply_finality_signatures(
        &mut self,
        finality_signatures: Vec<FinalitySignature>,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self.acquisition_state.with_signatures(
            finality_signatures,
            self.validator_matrix.clone(),
            self.should_fetch_execution_state,
        ) {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisitionError(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_global_state(
        &mut self,
        global_state: Digest,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .with_global_state(global_state, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisitionError(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_deploy(
        &mut self,
        deploy_hash: DeployHash,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .with_deploy(deploy_hash, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisitionError(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_execution_results(
        &mut self,
        deploy_hash: DeployHash,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(err) = self
            .acquisition_state
            .with_execution_results(deploy_hash, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisitionError(err));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_era_validators(
        &mut self,
        validator_matrix: Rc<ValidatorMatrix>,
    ) -> Result<(), Error> {
        self.validator_matrix = validator_matrix;
        self.touch();
        Ok(())
    }

    pub(crate) fn apply_peers(&mut self, peers: Vec<NodeId>) -> Result<(), Error> {
        peers
            .into_iter()
            .for_each(|peer| self.peer_list.register_peer(peer));
        self.touch();
        Ok(())
    }
}
