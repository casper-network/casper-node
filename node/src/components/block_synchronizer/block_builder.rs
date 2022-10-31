use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use datasize::DataSize;
use tracing::{debug, error};

use crate::components::block_synchronizer::{
    block_acquisition,
    block_acquisition::{BlockAcquisitionAction, BlockAcquisitionState},
};
use casper_hashing::Digest;
use casper_types::{EraId, TimeDiff, Timestamp};

use crate::{
    components::block_synchronizer::{
        block_acquisition::FinalitySignatureAcceptance,
        execution_results_acquisition::ExecutionResultsChecksum,
        peer_list::{PeerList, PeersStatus},
        signature_acquisition::SignatureAcquisition,
    },
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash, BlockHeader, DeployHash,
        DeployId, EraValidatorWeights, FinalitySignature, NodeId, SyncLeap, ValidatorMatrix,
    },
    NodeRng,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(super) enum Error {
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
pub(super) struct BlockBuilder {
    // imputed
    block_hash: BlockHash,
    should_fetch_execution_state: bool,
    requires_strict_finality: bool,
    peer_list: PeerList,

    // progress tracking
    last_progress: Timestamp,
    in_flight_latch: bool,

    // acquired state
    acquisition_state: BlockAcquisitionState,
    era_id: Option<EraId>,
    validator_weights: Option<EraValidatorWeights>,
}

impl BlockBuilder {
    pub(super) fn new(
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        requires_strict_finality: bool,
        max_simultaneous_peers: u32,
        peer_refresh_interval: TimeDiff,
    ) -> Self {
        BlockBuilder {
            block_hash,
            era_id: None,
            validator_weights: None,
            acquisition_state: BlockAcquisitionState::Initialized(
                block_hash,
                SignatureAcquisition::new(vec![]),
            ),
            peer_list: PeerList::new(max_simultaneous_peers, peer_refresh_interval),
            should_fetch_execution_state,
            requires_strict_finality,
            last_progress: Timestamp::now(),
            in_flight_latch: false,
        }
    }

    pub(super) fn new_from_sync_leap(
        sync_leap: &SyncLeap,
        validator_weights: EraValidatorWeights,
        peers: Vec<NodeId>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
        peer_refresh_interval: TimeDiff,
    ) -> Self {
        let (block_header, maybe_sigs) = sync_leap.highest_block_header();
        let block_hash = block_header.block_hash();
        let era_id = Some(block_header.era_id());
        let mut signature_acquisition =
            SignatureAcquisition::new(validator_weights.validator_public_keys().cloned().collect());
        if let Some(signatures) = maybe_sigs {
            for finality_signature in signatures.finality_signatures() {
                signature_acquisition.apply_signature(finality_signature);
            }
        }
        let acquisition_state = BlockAcquisitionState::HaveWeakFinalitySignatures(
            Box::new(block_header.clone()),
            signature_acquisition,
        );
        let mut peer_list = PeerList::new(max_simultaneous_peers, peer_refresh_interval);
        peers.iter().for_each(|p| peer_list.register_peer(*p));

        // we always require strict finality when synchronizing a block
        // via a sync leap response
        let requires_strict_finality = true;

        BlockBuilder {
            block_hash,
            era_id,
            validator_weights: Some(validator_weights),
            acquisition_state,
            peer_list,
            should_fetch_execution_state,
            requires_strict_finality,
            last_progress: Timestamp::now(),
            in_flight_latch: false,
        }
    }

    pub(super) fn abort(&mut self) {
        self.acquisition_state = BlockAcquisitionState::Fatal;
        self.flush_peers();
        self.touch();
    }

    pub(super) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        self.acquisition_state.block_height()
    }

    pub(super) fn last_progress_time(&self) -> Timestamp {
        self.last_progress
    }

    pub(super) fn in_flight_latch(&self) -> bool {
        self.in_flight_latch
    }

    pub(super) fn set_in_flight_latch(&mut self, in_flight_latch: bool) {
        self.in_flight_latch = in_flight_latch;
    }

    pub(super) fn is_fatal(&self) -> bool {
        self.acquisition_state == BlockAcquisitionState::Fatal
    }

    pub(super) fn is_finished(&self) -> bool {
        matches!(
            self.acquisition_state,
            BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
        )
    }

    pub(super) fn register_marked_complete(&mut self) {
        if let Err(error) = self.acquisition_state.register_marked_complete() {
            error!(%error, "register marked complete failed");
            self.abort()
        } else {
            self.touch();
        }
    }

    pub(super) fn dishonest_peers(&self) -> Vec<NodeId> {
        self.peer_list.dishonest_peers()
    }

    pub(super) fn disqualify_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.disqualify_peer(peer);
    }

    pub(super) fn promote_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.promote_peer(peer);
    }

    pub(super) fn demote_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.demote_peer(peer);
    }

    pub(super) fn flush_dishonest_peers(&mut self) {
        self.peer_list.flush_dishonest_peers();
    }

    pub(super) fn block_acquisition_action(&mut self, rng: &mut NodeRng) -> BlockAcquisitionAction {
        if self.in_flight_latch() {
            return BlockAcquisitionAction::noop();
        }
        match self.peer_list.need_peers() {
            PeersStatus::Sufficient => {
                debug!(
                    "BlockBuilder: sufficent peers for block_hash {}",
                    self.block_hash
                );
            }
            PeersStatus::Insufficient => {
                debug!(
                    "BlockBuilder: insufficient peers for block_hash {}",
                    self.block_hash
                );
                return BlockAcquisitionAction::peers(self.block_hash);
            }
            PeersStatus::Stale => {
                debug!("BlockBuilder: refreshing peers for {}", self.block_hash);
                return BlockAcquisitionAction::peers(self.block_hash);
            }
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
            Some(validator_weights) => validator_weights,
        };
        match self.acquisition_state.next_action(
            &self.peer_list,
            validator_weights,
            rng,
            self.should_fetch_execution_state,
        ) {
            Ok(ret) => ret,
            Err(err) => {
                error!(%err);
                self.abort();
                BlockAcquisitionAction::noop()
            }
        }
    }

    pub(super) fn register_era_validator_weights(&mut self, validator_matrix: &ValidatorMatrix) {
        if self.validator_weights.is_some() || self.era_id.is_none() {
            return;
        }

        if let Some(era_id) = self.era_id {
            if let Some(evw) = validator_matrix.validator_weights(era_id) {
                self.validator_weights = Some(evw);
            }
        }
    }

    pub(super) fn register_block_header(
        &mut self,
        block_header: BlockHeader,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let era_id = block_header.era_id();
        if let Err(error) = self.acquisition_state.register_header(block_header) {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisition(error));
        }
        self.era_id = Some(era_id);
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(super) fn register_block(
        &mut self,
        block: &Block,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .register_block(block, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisition(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .register_approvals_hashes(approvals_hashes, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisition(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Some(validator_weights) = self.validator_weights.to_owned() {
            match self
                .acquisition_state
                .register_finality_signature(finality_signature, validator_weights)
            {
                Err(error) => {
                    self.disqualify_peer(maybe_peer);
                    return Err(Error::BlockAcquisition(error));
                }
                Ok(FinalitySignatureAcceptance::NeededIt) => {
                    self.touch();
                    self.promote_peer(maybe_peer);
                }
                Ok(FinalitySignatureAcceptance::Noop) => {
                    // noop
                }
            }
            Ok(())
        } else {
            Err(Error::MissingValidatorWeights(self.block_hash))
        }
    }

    pub(super) fn register_global_state(&mut self, global_state: Digest) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .register_global_state(global_state, self.should_fetch_execution_state)
        {
            return Err(Error::BlockAcquisition(error));
        }
        self.touch();
        Ok(())
    }

    pub(super) fn register_execution_results_checksum(
        &mut self,
        execution_results_checksum: ExecutionResultsChecksum,
    ) -> Result<(), Error> {
        if let Err(err) = self.acquisition_state.register_execution_results_checksum(
            execution_results_checksum,
            self.should_fetch_execution_state,
        ) {
            return Err(Error::BlockAcquisition(err));
        }
        self.touch();
        Ok(())
    }

    pub(super) fn register_fetched_execution_results(
        &mut self,
        maybe_peer: Option<NodeId>,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
    ) -> Result<Option<HashMap<DeployHash, casper_types::ExecutionResult>>, Error> {
        match self.acquisition_state.register_execution_results_or_chunk(
            block_execution_results_or_chunk,
            self.should_fetch_execution_state,
        ) {
            Ok(maybe) => {
                self.touch();
                self.promote_peer(maybe_peer);
                Ok(maybe)
            }
            Err(error) => {
                // todo! - how to proceed when we receive incorrect chunks (for example,
                // `ChunksWithDifferentChecksum` for legacy blocks)? we probably
                // shouldn't disconnect from the peer, but logic to be discussed
                self.disqualify_peer(maybe_peer);
                Err(Error::BlockAcquisition(error))
            }
        }
    }

    pub(super) fn register_execution_results_stored_notification(&mut self) -> Result<(), Error> {
        if let Err(err) = self
            .acquisition_state
            .register_execution_results_stored_notification(self.should_fetch_execution_state)
        {
            self.abort();
            return Err(Error::BlockAcquisition(err));
        }
        self.touch();
        Ok(())
    }

    pub(super) fn register_deploy(
        &mut self,
        deploy_id: DeployId,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .register_deploy(deploy_id, self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisition(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    pub(super) fn register_peers(&mut self, peers: Vec<NodeId>) {
        peers.into_iter().for_each(|peer| {
            if !(self.is_finished() || self.is_fatal()) {
                self.peer_list.register_peer(peer)
            }
        });
        self.touch();
    }

    fn flush_peers(&mut self) {
        self.peer_list.flush();
    }

    fn touch(&mut self) {
        self.last_progress = Timestamp::now();
        self.set_in_flight_latch(false);
    }
}
