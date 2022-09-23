use std::{
    collections::{btree_map::Entry, BTreeMap, HashSet},
    env::var,
    fmt::{Display, Formatter},
    hash::Hash,
    rc::Rc,
    time::Duration,
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use openssl::pkey::Public;
use rand::{prelude::SliceRandom, seq::IteratorRandom, Rng};
use tracing::error;

use crate::components::block_synchronizer::{
    block_acquisition,
    block_acquisition::{BlockAcquisitionAction, BlockAcquisitionState},
};
use casper_hashing::Digest;
use casper_types::{system::auction::EraValidators, EraId, PublicKey, Timestamp, U512};

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::DeployAcquisition, need_next::NeedNext, peer_list::PeerList,
        signature_acquisition::SignatureAcquisition,
    },
    types::{
        Block, BlockAdded, BlockBody, BlockHash, BlockHeader, DeployHash, EraValidatorWeights,
        FinalitySignature, NodeId, SignatureWeight, SyncLeap, ValidatorMatrix,
    },
    utils::Source::Peer,
    NodeRng,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    BlockAcquisition(block_acquisition::Error),
    MissingValidatorWeights(BlockHash),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BlockAcquisition(err) => write!(f, "block acquisition error: {}", err),
            Error::MissingValidatorWeights(block_hash) => {
                write!(f, "missing validator weights for: {}", block_hash)
            }
        }
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockBuilder {
    // imputed
    block_hash: BlockHash,
    should_fetch_execution_state: bool,
    peer_list: PeerList,

    era_id: Option<EraId>,
    validator_weights: Option<EraValidatorWeights>,

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
        validator_weights: EraValidatorWeights,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) -> Self {
        let public_keys = validator_weights.validator_public_keys().cloned().collect();
        let peer_list = PeerList::new(max_simultaneous_peers);
        BlockBuilder {
            block_hash,
            era_id: Some(era_id),
            validator_weights: Some(validator_weights),
            acquisition_state: BlockAcquisitionState::Initialized(
                block_hash,
                SignatureAcquisition::new(public_keys),
            ),
            peer_list,
            should_fetch_execution_state,
            started: None,
            last_progress_time: None,
        }
    }

    pub(crate) fn new_leap(
        block_hash: BlockHash,
        sync_leap: SyncLeap,
        validator_weights: EraValidatorWeights,
        peers: Vec<NodeId>,
        fault_tolerance_fraction: Ratio<u64>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) -> Self {
        if let Some(fat_block_header) = sync_leap.highest_block_header() {
            let block_header = fat_block_header.block_header;
            let sigs = fat_block_header.block_signatures;
            let era_id = Some(block_header.era_id());
            let public_keys = sigs.proofs.into_iter().map(|(k, _)| k).collect_vec();
            let signature_acquisition = SignatureAcquisition::new(public_keys);
            let acquisition_state = BlockAcquisitionState::HaveBlockHeader(
                Box::new(block_header),
                signature_acquisition,
            );
            let mut peer_list = PeerList::new(max_simultaneous_peers);
            peers.iter().map(|p| peer_list.register_peer(*p));

            BlockBuilder {
                block_hash,
                era_id,
                validator_weights: Some(validator_weights),
                acquisition_state,
                peer_list,
                should_fetch_execution_state,
                started: None,
                last_progress_time: None,
            }
        } else {
            BlockBuilder::new_minimal(
                block_hash,
                should_fetch_execution_state,
                max_simultaneous_peers,
            )
        }
    }

    pub(crate) fn new_minimal(
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) -> Self {
        BlockBuilder {
            block_hash,
            era_id: None,
            validator_weights: None,
            acquisition_state: BlockAcquisitionState::Initialized(
                block_hash,
                SignatureAcquisition::new(vec![]),
            ),
            peer_list: PeerList::new(max_simultaneous_peers),
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

    pub(crate) fn block_hash(&self) -> BlockHash {
        self.block_hash
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

    pub(crate) fn need_next(&mut self, rng: &mut NodeRng) -> BlockAcquisitionAction {
        if self.peer_list.need_peers() {
            return BlockAcquisitionAction::peers();
        }
        let era_id = match self.era_id {
            None => {
                return BlockAcquisitionAction::block_header(&self.peer_list, rng, self.block_hash);
            }
            Some(era_id) => era_id,
        };
        let validator_weights = match &self.validator_weights {
            None => {
                return BlockAcquisitionAction::era_validators(era_id);
            }
            Some(vm) => vm,
        };
        match self.acquisition_state.next_action(
            &self.peer_list,
            validator_weights,
            rng,
            self.should_fetch_execution_state,
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
            return Err(Error::BlockAcquisition(error));
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
            .with_body(block, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisition(error));
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
        if let Some(validator_weights) = self.validator_weights.to_owned() {
            if let Err(error) = self.acquisition_state.with_signatures(
                finality_signatures,
                validator_weights,
                self.should_fetch_execution_state,
            ) {
                self.disqualify_peer(maybe_peer);
                return Err(Error::BlockAcquisition(error));
            }
            self.touch();
            self.promote_peer(maybe_peer);
            Ok(())
        } else {
            Err(Error::MissingValidatorWeights(self.block_hash))
        }
    }

    // TODO: this needs to be called from somewhere, ya?
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
            return Err(Error::BlockAcquisition(error));
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
            return Err(Error::BlockAcquisition(error));
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
            return Err(Error::BlockAcquisition(err));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(crate) fn apply_era_validators(
        &mut self,
        validator_weights: EraValidatorWeights,
    ) -> Result<(), Error> {
        self.validator_weights = Some(validator_weights);
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
