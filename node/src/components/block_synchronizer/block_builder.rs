use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    time::Instant,
};

use datasize::DataSize;
use tracing::{debug, error, trace, warn};

use casper_hashing::Digest;
use casper_types::{EraId, TimeDiff, Timestamp};

use super::{
    block_acquisition::{Acceptance, BlockAcquisitionState},
    block_acquisition_action::BlockAcquisitionAction,
    execution_results_acquisition::{self, ExecutionResultsChecksum},
    peer_list::{PeerList, PeersStatus},
    signature_acquisition::SignatureAcquisition,
    BlockAcquisitionError,
};
use crate::{
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash, BlockHeader,
        BlockSignatures, DeployHash, DeployId, EraValidatorWeights, FinalitySignature, NodeId,
        ValidatorMatrix,
    },
    NodeRng,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(super) enum Error {
    BlockAcquisition(BlockAcquisitionError),
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
    sync_start: Instant,
    last_progress: Timestamp,
    in_flight_latch: Option<Timestamp>,

    // acquired state
    acquisition_state: BlockAcquisitionState,
    era_id: Option<EraId>,
    validator_weights: Option<EraValidatorWeights>,
}

impl Display for BlockBuilder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "is_historical: {:?}, has_validators: {:?}, block builder: {}",
            self.should_fetch_execution_state,
            self.validator_weights.is_some(),
            self.acquisition_state
        )
    }
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
            sync_start: Instant::now(),
            last_progress: Timestamp::now(),
            in_flight_latch: None,
        }
    }

    pub(super) fn new_from_sync_leap(
        block_header: &BlockHeader,
        maybe_sigs: Option<&BlockSignatures>,
        validator_weights: EraValidatorWeights,
        peers: Vec<NodeId>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
        peer_refresh_interval: TimeDiff,
    ) -> Self {
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
            sync_start: Instant::now(),
            last_progress: Timestamp::now(),
            in_flight_latch: None,
        }
    }

    pub(super) fn abort(&mut self) {
        self.acquisition_state =
            BlockAcquisitionState::Failed(self.block_hash, self.block_height());
        self.flush_peers();
        self.touch();
    }

    pub(crate) fn block_acquisition_state(&self) -> &BlockAcquisitionState {
        &self.acquisition_state
    }

    pub(super) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    pub(super) fn maybe_block(&self) -> Option<Box<Block>> {
        self.acquisition_state.maybe_block()
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        self.acquisition_state.block_height()
    }

    pub(super) fn block_height_and_era(&self) -> Option<(u64, EraId)> {
        if let Some(block_height) = self.acquisition_state.block_height() {
            if let Some(evw) = &self.validator_weights {
                return Some((block_height, evw.era_id()));
            }
        }
        None
    }

    pub(super) fn should_fetch_execution_state(&self) -> bool {
        self.should_fetch_execution_state
    }

    pub(super) fn sync_start_time(&self) -> Instant {
        self.sync_start
    }

    pub(super) fn last_progress_time(&self) -> Timestamp {
        self.last_progress
    }

    pub(super) fn in_flight_latch(&mut self) -> Option<Timestamp> {
        if let Some(timestamp) = self.in_flight_latch {
            // we put a latch on ourselves the first time we signal we need something specific
            // if asked again before we get what we need, and latch_reset_interval has not passed,
            // we signal we need nothing to avoid spamming redundant asks
            //
            // if latch_reset_interval has passed, we reset the latch and ask again.

            // !todo move reset interval to config
            let latch_reset_interval = TimeDiff::from_seconds(5);
            if Timestamp::now().saturating_diff(timestamp) > latch_reset_interval {
                self.in_flight_latch = None;
            }
        }
        self.in_flight_latch
    }

    pub(super) fn set_in_flight_latch(&mut self) {
        self.in_flight_latch = Some(Timestamp::now());
    }

    pub(super) fn is_failed(&self) -> bool {
        matches!(self.acquisition_state, BlockAcquisitionState::Failed(_, _))
    }

    pub(super) fn is_finished(&self) -> bool {
        matches!(
            self.acquisition_state,
            BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
        )
    }

    pub(super) fn register_block_execution_not_enqueued(&mut self) {
        warn!("failed to enqueue block for execution");
        // reset latch and try again
        self.touch();
    }

    pub(super) fn register_block_execution_enqueued(&mut self) {
        if let Err(error) = self
            .acquisition_state
            .register_block_execution_enqueued(self.should_fetch_execution_state)
        {
            error!(%error, "register block execution enqueued failed");
            self.abort()
        } else {
            self.touch();
        }
    }

    pub(super) fn register_marked_complete(&mut self) {
        if let Err(error) = self
            .acquisition_state
            .register_marked_complete(self.should_fetch_execution_state)
        {
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
        match self.peer_list.need_peers() {
            PeersStatus::Sufficient => {
                trace!(
                    "BlockBuilder: sufficient peers for block_hash {}",
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
                error!(%err, "BlockBuilder: attempt to determine next action resulted in error.");
                self.abort();
                BlockAcquisitionAction::need_nothing(self.block_hash)
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
        let acceptance = self.acquisition_state.register_block_header(block_header);
        self.handle_acceptance(maybe_peer, acceptance)?;
        self.era_id = Some(era_id);
        Ok(())
    }

    pub(super) fn register_block(
        &mut self,
        block: &Block,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let acceptance = self
            .acquisition_state
            .register_block(block, self.should_fetch_execution_state);
        self.handle_acceptance(maybe_peer, acceptance)
    }

    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let acceptance = self
            .acquisition_state
            .register_approvals_hashes(approvals_hashes, self.should_fetch_execution_state);
        self.handle_acceptance(maybe_peer, acceptance)
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let validator_weights = self
            .validator_weights
            .as_ref()
            .ok_or(Error::MissingValidatorWeights(self.block_hash))?;
        let acceptance = self
            .acquisition_state
            .register_finality_signature(finality_signature, validator_weights);
        self.handle_acceptance(maybe_peer, acceptance)
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
            Err(BlockAcquisitionError::ExecutionResults(error)) => {
                match error {
                    // late response - not considered an error
                    execution_results_acquisition::Error::AttemptToApplyDataAfterCompleted { .. } => {
                        debug!(%error, "late block_execution_results_or_chunk response");
                        return Ok(None);
                    }
                    // programmer error
                    execution_results_acquisition::Error::BlockHashMismatch { .. }
                    | execution_results_acquisition::Error::InvalidAttemptToApplyChecksum { .. }
                    | execution_results_acquisition::Error::AttemptToApplyDataWhenMissingChecksum { .. } => {},
                    // malicious peer if checksum is available.
                    execution_results_acquisition::Error::ChunkCountMismatch { .. } => {
                        let is_checkable = match &self.acquisition_state {
                            BlockAcquisitionState::HaveGlobalState(
                                _,
                                _,
                                _,
                                execution_results_acquisition,
                            ) => execution_results_acquisition.is_checkable(),
                            _ => false,
                        };
                        if is_checkable {
                            self.disqualify_peer(maybe_peer);
                        }
                    }
                    // malicious peer
                    execution_results_acquisition::Error::InvalidChunkCount { .. }
                    | execution_results_acquisition::Error::ChecksumMismatch { .. }
                    | execution_results_acquisition::Error::FailedToDeserialize { .. }
                    | execution_results_acquisition::Error::ExecutionResultToDeployHashLengthDiscrepancy { .. } => {
                        self.disqualify_peer(maybe_peer);
                    }
                    // checksum unavailable, so unknown if this peer is malicious
                    execution_results_acquisition::Error::ChunksWithDifferentChecksum { .. } => {}
                }
                Err(Error::BlockAcquisition(
                    BlockAcquisitionError::ExecutionResults(error),
                ))
            }
            Err(error) => {
                error!(%error, "unexpected error");
                Ok(None)
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
        let acceptance = self
            .acquisition_state
            .register_deploy(deploy_id, self.should_fetch_execution_state);
        self.handle_acceptance(maybe_peer, acceptance)
    }

    pub(super) fn register_peers(&mut self, peers: Vec<NodeId>) {
        peers.into_iter().for_each(|peer| {
            if !(self.is_finished() || self.is_failed()) {
                self.peer_list.register_peer(peer)
            }
        });
        self.touch();
    }

    fn handle_acceptance(
        &mut self,
        maybe_peer: Option<NodeId>,
        acceptance: Result<Option<Acceptance>, BlockAcquisitionError>,
    ) -> Result<(), Error> {
        match acceptance {
            Ok(Some(Acceptance::NeededIt)) => {
                self.touch();
                self.promote_peer(maybe_peer);
            }
            Ok(Some(Acceptance::HadIt)) | Ok(None) => (),
            Err(error) => {
                self.disqualify_peer(maybe_peer);
                return Err(Error::BlockAcquisition(error));
            }
        }
        Ok(())
    }

    fn flush_peers(&mut self) {
        self.peer_list.flush();
    }

    fn touch(&mut self) {
        self.last_progress = Timestamp::now();
        self.in_flight_latch = None;
    }
}
