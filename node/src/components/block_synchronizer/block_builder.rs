#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    time::Instant,
};

use casper_execution_engine::{core::engine_state, storage::trie::TrieRaw};
use datasize::DataSize;
use tracing::{debug, error, trace, warn};

use casper_hashing::Digest;
use casper_types::{system::auction::EraValidators, EraId, PublicKey, TimeDiff, Timestamp};

use super::{
    block_acquisition::{Acceptance, BlockAcquisitionState, NextActionConfig},
    block_acquisition_action::BlockAcquisitionAction,
    event::EraValidatorsGetError,
    execution_results_acquisition::{self, ExecutionResultsChecksum},
    peer_list::{PeerList, PeersStatus},
    signature_acquisition::SignatureAcquisition,
    BlockAcquisitionError,
};
use crate::{
    types::{
        chainspec::LegacyRequiredFinality, ApprovalsHashes, Block, BlockExecutionResultsOrChunk,
        BlockHash, BlockHeader, BlockSignatures, DeployHash, DeployId, EraValidatorWeights,
        FinalitySignature, NodeId, TrieOrChunk, ValidatorMatrix,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, DataSize)]
enum ExecutionProgress {
    Idle,
    Started,
    Done,
}

impl ExecutionProgress {
    fn start(self) -> Option<Self> {
        match self {
            Self::Idle => Some(Self::Started),
            _ => None,
        }
    }

    fn finish(self) -> Option<Self> {
        match self {
            Self::Started => Some(Self::Done),
            _ => None,
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
    get_evw_from_global_state: bool,

    // progress tracking
    sync_start: Instant,
    execution_progress: ExecutionProgress,
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
        get_evw_from_global_state: bool,
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
            execution_progress: ExecutionProgress::Idle,
            last_progress: Timestamp::now(),
            in_flight_latch: None,
            get_evw_from_global_state,
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
                let _ =
                    signature_acquisition.apply_signature(finality_signature, &validator_weights);
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
            execution_progress: ExecutionProgress::Idle,
            last_progress: Timestamp::now(),
            in_flight_latch: None,
            get_evw_from_global_state: false,
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
            let latch_reset_interval = TimeDiff::from_seconds(50000);
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

    pub(super) fn is_executing(&self) -> bool {
        matches!(self.execution_progress, ExecutionProgress::Started)
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
            match (
                self.should_fetch_execution_state,
                self.execution_progress.start(),
            ) {
                (true, _) => {
                    let block_hash = self.block_hash();
                    error!(%block_hash, "invalid attempt to start block execution on historical block");
                    self.abort();
                }
                (false, None) => {
                    let block_hash = self.block_hash();
                    error!(%block_hash, "invalid attempt to start block execution");
                    self.abort();
                }
                (false, Some(executing_progress)) => {
                    self.touch();
                    self.execution_progress = executing_progress;
                }
            }
        }
    }

    pub(super) fn register_block_executed(&mut self) {
        match (
            self.should_fetch_execution_state,
            self.execution_progress.finish(),
        ) {
            (true, _) => {
                let block_hash = self.block_hash();
                error!(%block_hash, "invalid attempt to finish block execution on historical block");
                self.abort();
            }
            (false, None) => {
                let block_hash = self.block_hash();
                error!(%block_hash, "invalid attempt to finish block execution");
                self.abort();
            }
            (false, Some(executing_progress)) => {
                self.touch();
                self.execution_progress = executing_progress;
            }
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
        debug!(?peer, "disqualify_peer");
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

    pub(super) fn block_acquisition_action(
        &mut self,
        rng: &mut NodeRng,
        max_simultaneous_peers: usize,
        max_parallel_trie_fetches: usize,
        legacy_required_finality: LegacyRequiredFinality,
    ) -> BlockAcquisitionAction {
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
                // if we don't have the era_id, we only have block_hash, thus get block_header
                return BlockAcquisitionAction::block_header(&self.peer_list, rng, self.block_hash);
            }
            Some(era_id) => era_id,
        };
        let validator_weights = match &self.validator_weights {
            None => {
                if self.should_fetch_execution_state() && self.get_evw_from_global_state {
                    None
                } else {
                    return BlockAcquisitionAction::era_validators(&self.peer_list, rng, era_id);
                }
            }
            Some(validator_weights) => {
                if validator_weights.is_empty() {
                    return BlockAcquisitionAction::era_validators(&self.peer_list, rng, era_id);
                }
                Some(validator_weights)
            }
        };
        match self.acquisition_state.next_action(
            &self.peer_list,
            validator_weights,
            rng,
            self.should_fetch_execution_state,
            NextActionConfig {
                legacy_required_finality,
                max_simultaneous_peers,
                max_parallel_trie_fetches,
            },
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

    pub(super) fn register_finality_signature_pending(&mut self, validator: PublicKey) {
        self.acquisition_state
            .register_finality_signature_pending(validator);
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
        let acceptance = self.acquisition_state.register_finality_signature(
            finality_signature,
            validator_weights,
            self.should_fetch_execution_state,
        );
        self.handle_acceptance(maybe_peer, acceptance)
    }

    pub(super) fn register_trie_or_chunk(
        &mut self,
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_or_chunk: TrieOrChunk,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let acceptance = self.acquisition_state.register_trie_or_chunk(
            state_root_hash,
            trie_hash,
            trie_or_chunk,
        );
        self.handle_acceptance(maybe_peer, acceptance)
    }

    pub(super) fn register_trie_fetch_error(
        &mut self,
        state_root_hash: Digest,
        trie_hash: Digest,
    ) -> Result<(), Error> {
        let acceptance = self
            .acquisition_state
            .register_trie_or_chunk_fetch_error(state_root_hash, trie_hash);
        self.handle_acceptance(None, acceptance)
    }

    pub(super) fn register_put_trie(
        &mut self,
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_raw: TrieRaw,
        put_trie_result: Result<Digest, engine_state::Error>,
    ) -> Result<(), Error> {
        let acceptance = self.acquisition_state.register_put_trie(
            state_root_hash,
            trie_hash,
            trie_raw,
            put_trie_result,
        );
        self.handle_acceptance(None, acceptance)
    }

    pub(super) fn register_block_header_requested_from_storage(
        &mut self,
        block_header: Option<BlockHeader>,
    ) -> Result<(), Error> {
        let acceptance = self
            .acquisition_state
            .register_block_header_requested_from_storage(block_header);
        self.handle_acceptance(None, acceptance)
    }

    pub(super) fn register_era_validators_from_contract_runtime(
        &mut self,
        from_state_root_hash: Digest,
        era_validators: Result<EraValidators, EraValidatorsGetError>,
    ) -> Result<(), Error> {
        let acceptance = self
            .acquisition_state
            .register_era_validators_from_contract_runtime(from_state_root_hash, era_validators);
        self.handle_acceptance(None, acceptance)
    }

    pub(super) fn register_execution_results_checksum(
        &mut self,
        execution_results_checksum: ExecutionResultsChecksum,
    ) -> Result<(), Error> {
        debug!(block_hash=%self.block_hash, "register_execution_results_checksum");
        if let Err(err) = self.acquisition_state.register_execution_results_checksum(
            execution_results_checksum,
            self.should_fetch_execution_state,
        ) {
            debug!(block_hash=%self.block_hash, %err, "register_execution_results_checksum: Error::BlockAcquisition");
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
        debug!(block_hash=%self.block_hash, "register_fetched_execution_results");
        match self.acquisition_state.register_execution_results_or_chunk(
            block_execution_results_or_chunk,
            self.should_fetch_execution_state,
        ) {
            Ok(maybe) => {
                debug!("register_fetched_execution_results: Ok(maybe)");
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
                    | execution_results_acquisition::Error::AttemptToApplyDataWhenMissingChecksum { .. } => {
                        debug!("register_fetched_execution_results: BlockHashMismatch | InvalidAttemptToApplyChecksum | AttemptToApplyDataWhenMissingChecksum");
                    },
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
                        debug!(is_checkable, "register_fetched_execution_results: ChunkCountMismatch");
                        if is_checkable {
                            self.disqualify_peer(maybe_peer);
                        }
                    }
                    // malicious peer
                    execution_results_acquisition::Error::InvalidChunkCount { .. }
                    | execution_results_acquisition::Error::ChecksumMismatch { .. }
                    | execution_results_acquisition::Error::FailedToDeserialize { .. }
                    | execution_results_acquisition::Error::ExecutionResultToDeployHashLengthDiscrepancy { .. } => {
                        debug!("register_fetched_execution_results: InvalidChunkCount | ChecksumMismatch | FailedToDeserialize | ExecutionResultToDeployHashLengthDiscrepancy");
                        self.disqualify_peer(maybe_peer);
                    }
                    // checksum unavailable, so unknown if this peer is malicious
                    execution_results_acquisition::Error::ChunksWithDifferentChecksum { .. } => {
                        debug!("register_fetched_execution_results: ChunksWithDifferentChecksum");

                    }
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
        debug!(block_hash=%self.block_hash, "register_execution_results_stored_notification");
        if let Err(err) = self
            .acquisition_state
            .register_execution_results_stored_notification(self.should_fetch_execution_state)
        {
            debug!(block_hash=%self.block_hash, "register_execution_results_stored_notification: abort");
            self.abort();
            return Err(Error::BlockAcquisition(err));
        }
        self.touch();
        Ok(())
    }

    pub(super) fn register_pending_put_tries(
        &mut self,
        state_root_hash: Digest,
        put_tries_in_progress: HashSet<Digest>,
    ) {
        self.acquisition_state
            .register_pending_put_tries(state_root_hash, put_tries_in_progress);
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
