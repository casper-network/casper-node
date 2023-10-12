mod latch;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    time::Instant,
};

use datasize::DataSize;
use tracing::{debug, error, trace, warn};

use casper_types::{
    execution::ExecutionResult, Block, BlockHash, BlockHeader, BlockSignatures, DeployHash,
    DeployId, Digest, EraId, FinalitySignature, LegacyRequiredFinality, ProtocolVersion, PublicKey,
    TimeDiff, Timestamp,
};

use super::{
    block_acquisition::{Acceptance, BlockAcquisitionState, RegisterExecResultsOutcome},
    block_acquisition_action::BlockAcquisitionAction,
    execution_results_acquisition::{self, ExecutionResultsChecksum},
    peer_list::{PeerList, PeersStatus},
    signature_acquisition::SignatureAcquisition,
    BlockAcquisitionError,
};
use crate::{
    components::block_synchronizer::block_builder::latch::Latch,
    types::{
        ApprovalsHashes, BlockExecutionResultsOrChunk, EraValidatorWeights, ExecutableBlock,
        NodeId, ValidatorMatrix,
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
    strict_finality_protocol_version: ProtocolVersion,
    peer_list: PeerList,

    // progress tracking
    sync_start: Instant,
    execution_progress: ExecutionProgress,
    last_progress: Timestamp,
    latch: Latch,

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
        max_simultaneous_peers: u8,
        peer_refresh_interval: TimeDiff,
        legacy_required_finality: LegacyRequiredFinality,
        strict_finality_protocol_version: ProtocolVersion,
    ) -> Self {
        BlockBuilder {
            block_hash,
            era_id: None,
            validator_weights: None,
            acquisition_state: BlockAcquisitionState::Initialized(
                block_hash,
                SignatureAcquisition::new(vec![], legacy_required_finality),
            ),
            peer_list: PeerList::new(max_simultaneous_peers, peer_refresh_interval),
            should_fetch_execution_state,
            strict_finality_protocol_version,
            sync_start: Instant::now(),
            execution_progress: ExecutionProgress::Idle,
            last_progress: Timestamp::now(),
            latch: Latch::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new_from_sync_leap(
        block_header: BlockHeader,
        maybe_sigs: Option<&BlockSignatures>,
        validator_weights: EraValidatorWeights,
        peers: Vec<NodeId>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u8,
        peer_refresh_interval: TimeDiff,
        legacy_required_finality: LegacyRequiredFinality,
        strict_finality_protocol_version: ProtocolVersion,
    ) -> Self {
        let block_hash = block_header.block_hash();
        let era_id = Some(block_header.era_id());
        let mut signature_acquisition = SignatureAcquisition::new(
            validator_weights.validator_public_keys().cloned().collect(),
            legacy_required_finality,
        );
        if let Some(signatures) = maybe_sigs {
            for finality_signature in signatures.finality_signatures() {
                let _ =
                    signature_acquisition.apply_signature(finality_signature, &validator_weights);
            }
        }
        let acquisition_state = BlockAcquisitionState::HaveWeakFinalitySignatures(
            Box::new(block_header),
            signature_acquisition,
        );
        let mut peer_list = PeerList::new(max_simultaneous_peers, peer_refresh_interval);
        peers.iter().for_each(|p| peer_list.register_peer(*p));

        BlockBuilder {
            block_hash,
            era_id,
            validator_weights: Some(validator_weights),
            acquisition_state,
            peer_list,
            should_fetch_execution_state,
            strict_finality_protocol_version,
            sync_start: Instant::now(),
            execution_progress: ExecutionProgress::Idle,
            last_progress: Timestamp::now(),
            latch: Latch::default(),
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

    #[cfg(test)]
    pub(crate) fn set_block_acquisition_state(&mut self, state: BlockAcquisitionState) {
        self.acquisition_state = state
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

    #[cfg(test)]
    pub fn latched(&self) -> bool {
        self.latch.count() > 0
    }

    #[cfg(test)]
    pub fn latch_count(&self) -> u8 {
        self.latch.count()
    }

    pub(super) fn check_latch(&mut self, interval: TimeDiff) -> bool {
        self.latch.check_latch(interval, Timestamp::now())
    }

    /// Increments the latch counter by 1.
    pub(super) fn latch(&mut self) {
        self.latch.increment(1);
    }

    pub(super) fn latch_by(&mut self, count: usize) {
        self.latch.increment(count as u8);
    }

    /// Decrements the latch counter.
    pub(super) fn latch_decrement(&mut self) {
        self.latch.decrement(1);
    }

    pub(super) fn is_failed(&self) -> bool {
        matches!(self.acquisition_state, BlockAcquisitionState::Failed(_, _))
    }

    pub(super) fn is_finished(&self) -> bool {
        match self.acquisition_state {
            BlockAcquisitionState::Initialized(_, _)
            | BlockAcquisitionState::HaveBlockHeader(_, _)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveBlock(_, _, _)
            | BlockAcquisitionState::HaveGlobalState(_, _, _, _)
            | BlockAcquisitionState::HaveAllExecutionResults(_, _, _, _)
            | BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
            | BlockAcquisitionState::HaveAllDeploys(_, _)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
            | BlockAcquisitionState::HaveFinalizedBlock(_, _, _)
            | BlockAcquisitionState::Failed(_, _) => {
                //TODO: does failed also mean finished?
                false
            }
            BlockAcquisitionState::Complete(_) => true,
        }
    }

    pub(super) fn is_executing(&self) -> bool {
        matches!(self.execution_progress, ExecutionProgress::Started)
    }

    pub(super) fn execution_unattempted(&self) -> bool {
        matches!(self.execution_progress, ExecutionProgress::Idle)
    }

    pub(super) fn register_block_execution_enqueued(&mut self) {
        if self.should_fetch_execution_state {
            let block_hash = self.block_hash();
            error!(%block_hash, "invalid attempt to enqueue historical block for execution");
            self.abort();
            return;
        }

        if let Err(error) = self.acquisition_state.register_block_execution_enqueued() {
            error!(%error, "register block execution enqueued failed");
            self.abort()
        } else {
            self.touch();
        }

        match self.execution_progress.start() {
            None => {
                let block_hash = self.block_hash();
                warn!(%block_hash, "invalid attempt to start block execution");
            }
            Some(executing_progress) => {
                self.touch();
                self.execution_progress = executing_progress;
            }
        }
    }

    pub(super) fn register_made_finalized_block(&mut self, executable_block: ExecutableBlock) {
        if let Err(error) = self
            .acquisition_state
            .register_made_finalized_block(self.should_fetch_execution_state, executable_block)
        {
            error!(%error, "register finalized block failed");
            self.abort()
        } else {
            self.touch();
        }
    }

    pub(super) fn register_block_executed(&mut self) {
        if let Err(error) = self
            .acquisition_state
            .register_block_executed(self.should_fetch_execution_state)
        {
            error!(%error, "register block executed failed");
            self.abort()
        } else {
            if self.should_fetch_execution_state {
                let block_hash = self.block_hash();
                error!(%block_hash, "invalid attempt to finish block execution on historical block");
                self.abort();
            }

            match self.execution_progress.finish() {
                None => {
                    let block_hash = self.block_hash();
                    warn!(%block_hash, "invalid attempt to finish block execution");
                }
                Some(executing_progress) => {
                    self.touch();
                    self.execution_progress = executing_progress;
                }
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

    pub(super) fn disqualify_peer(&mut self, peer: NodeId) {
        debug!(?peer, "disqualify_peer");
        self.peer_list.disqualify_peer(peer);
    }

    pub(super) fn promote_peer(&mut self, peer: NodeId) {
        self.peer_list.promote_peer(peer);
    }

    pub(super) fn demote_peer(&mut self, peer: NodeId) {
        self.peer_list.demote_peer(peer);
    }

    pub(super) fn flush_dishonest_peers(&mut self) {
        self.peer_list.flush_dishonest_peers();
    }

    pub(super) fn block_acquisition_action(
        &mut self,
        rng: &mut NodeRng,
        max_simultaneous_peers: u8,
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
                return BlockAcquisitionAction::era_validators(&self.peer_list, rng, era_id);
            }
            Some(validator_weights) => {
                if validator_weights.is_empty() {
                    return BlockAcquisitionAction::era_validators(&self.peer_list, rng, era_id);
                }
                validator_weights
            }
        };
        match self.acquisition_state.next_action(
            &self.peer_list,
            validator_weights,
            rng,
            self.should_fetch_execution_state,
            max_simultaneous_peers,
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
                self.touch();
            }
        }
    }

    pub(super) fn waiting_for_block_header(&self) -> bool {
        match &self.acquisition_state {
            BlockAcquisitionState::Initialized(..) => true,
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
            | BlockAcquisitionState::Complete(..) => false,
        }
    }

    pub(super) fn register_block_header(
        &mut self,
        block_header: BlockHeader,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let was_waiting_for_block_header = self.waiting_for_block_header();

        let era_id = block_header.era_id();
        let acceptance = self.acquisition_state.register_block_header(
            block_header,
            self.strict_finality_protocol_version,
            self.should_fetch_execution_state,
        );
        self.handle_acceptance(maybe_peer, acceptance, was_waiting_for_block_header)?;
        self.era_id = Some(era_id);
        Ok(())
    }

    pub(super) fn waiting_for_block(&self) -> bool {
        match &self.acquisition_state {
            BlockAcquisitionState::HaveWeakFinalitySignatures(..) => true,
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
            | BlockAcquisitionState::Complete(..) => false,
        }
    }

    pub(super) fn register_block(
        &mut self,
        block: Block,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let was_waiting_for_block = self.waiting_for_block();
        let acceptance = self
            .acquisition_state
            .register_block(block, self.should_fetch_execution_state);
        self.handle_acceptance(maybe_peer, acceptance, was_waiting_for_block)
    }

    pub(super) fn waiting_for_approvals_hashes(&self) -> bool {
        match &self.acquisition_state {
            BlockAcquisitionState::HaveBlock(..) if !self.should_fetch_execution_state => true,
            BlockAcquisitionState::HaveAllExecutionResults(..)
                if self.should_fetch_execution_state =>
            {
                true
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveApprovalsHashes(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => false,
        }
    }

    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let was_waiting_for_approvals_hashes = self.waiting_for_approvals_hashes();
        let acceptance = self
            .acquisition_state
            .register_approvals_hashes(approvals_hashes, self.should_fetch_execution_state);
        self.handle_acceptance(maybe_peer, acceptance, was_waiting_for_approvals_hashes)
    }

    pub(super) fn register_finality_signature_pending(&mut self, validator: PublicKey) {
        self.acquisition_state
            .register_finality_signature_pending(validator);
    }

    pub(super) fn switch_to_have_strict_finality(
        &mut self,
        block_hash: BlockHash,
    ) -> Result<(), Error> {
        match self
            .acquisition_state
            .switch_to_have_strict_finality(block_hash, self.should_fetch_execution_state)
        {
            Ok(()) => {
                self.touch();
                Ok(())
            }
            Err(error) => {
                self.abort();
                Err(Error::BlockAcquisition(error))
            }
        }
    }

    pub(super) fn waiting_for_signatures(&self) -> bool {
        self.acquisition_state
            .actively_acquiring_signatures(self.should_fetch_execution_state)
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let was_waiting_for_sigs = self.waiting_for_signatures();
        let validator_weights = self
            .validator_weights
            .as_ref()
            .ok_or(Error::MissingValidatorWeights(self.block_hash))?;
        let acceptance = self.acquisition_state.register_finality_signature(
            finality_signature,
            validator_weights,
            self.should_fetch_execution_state,
        );
        self.handle_acceptance(maybe_peer, acceptance, was_waiting_for_sigs)
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

    pub(super) fn waiting_for_execution_results(&self) -> bool {
        match &self.acquisition_state {
            BlockAcquisitionState::HaveGlobalState(..) if self.should_fetch_execution_state => true,
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
            | BlockAcquisitionState::Complete(..) => false,
        }
    }

    pub(super) fn register_fetched_execution_results(
        &mut self,
        maybe_peer: Option<NodeId>,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
    ) -> Result<Option<HashMap<DeployHash, ExecutionResult>>, Error> {
        debug!(block_hash=%self.block_hash, "register_fetched_execution_results");
        let was_waiting_for_execution_results = self.waiting_for_execution_results();
        match self.acquisition_state.register_execution_results_or_chunk(
            block_execution_results_or_chunk,
            self.should_fetch_execution_state,
        ) {
            Ok(RegisterExecResultsOutcome {
                exec_results,
                acceptance,
            }) => {
                debug!(
                    ?acceptance,
                    "register_fetched_execution_results: Ok(RegisterExecResultsOutcome)"
                );
                self.handle_acceptance(
                    maybe_peer,
                    Ok(acceptance),
                    was_waiting_for_execution_results,
                )?;
                Ok(exec_results)
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
                    | execution_results_acquisition::Error::AttemptToApplyDataWhenMissingChecksum { .. }
                    | execution_results_acquisition::Error::InvalidOutcomeFromApplyingChunk { .. }
                    => {
                        if was_waiting_for_execution_results {
                            self.latch_decrement();
                        }
                        debug!(
                            "register_fetched_execution_results: BlockHashMismatch | \
                            InvalidAttemptToApplyChecksum | AttemptToApplyDataWhenMissingChecksum \
                            | InvalidOutcomeFromApplyingChunk"
                        );
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
                            if let Some(peer) = maybe_peer {
                                self.disqualify_peer(peer);
                            }
                        }
                        if was_waiting_for_execution_results {
                            self.latch_decrement();
                        }
                    }
                    // malicious peer
                    execution_results_acquisition::Error::InvalidChunkCount { .. }
                    | execution_results_acquisition::Error::ChecksumMismatch { .. }
                    | execution_results_acquisition::Error::FailedToDeserialize { .. }
                    | execution_results_acquisition::Error::ExecutionResultToDeployHashLengthDiscrepancy { .. } => {
                        debug!("register_fetched_execution_results: InvalidChunkCount | ChecksumMismatch | FailedToDeserialize | ExecutionResultToDeployHashLengthDiscrepancy");
                        if let Some(peer) = maybe_peer {
                            self.disqualify_peer(peer);
                        }
                        if was_waiting_for_execution_results {
                            self.latch_decrement();
                        }
                    }
                    // checksum unavailable, so unknown if this peer is malicious
                    execution_results_acquisition::Error::ChunksWithDifferentChecksum { .. } => {
                        debug!("register_fetched_execution_results: ChunksWithDifferentChecksum");
                        if was_waiting_for_execution_results {
                            self.latch_decrement();
                        }
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

    pub(super) fn waiting_for_deploys(&self) -> bool {
        match &self.acquisition_state {
            BlockAcquisitionState::HaveApprovalsHashes(_, _, deploys) => {
                deploys.needs_deploy().is_some()
            }
            BlockAcquisitionState::HaveAllExecutionResults(_, _, deploys, checksum)
                if self.should_fetch_execution_state =>
            {
                if !checksum.is_checkable() {
                    deploys.needs_deploy().is_some()
                } else {
                    false
                }
            }
            BlockAcquisitionState::Initialized(..)
            | BlockAcquisitionState::HaveBlockHeader(..)
            | BlockAcquisitionState::HaveWeakFinalitySignatures(..)
            | BlockAcquisitionState::HaveBlock(..)
            | BlockAcquisitionState::HaveGlobalState(..)
            | BlockAcquisitionState::HaveAllExecutionResults(..)
            | BlockAcquisitionState::HaveAllDeploys(..)
            | BlockAcquisitionState::HaveStrictFinalitySignatures(..)
            | BlockAcquisitionState::HaveFinalizedBlock(..)
            | BlockAcquisitionState::Failed(..)
            | BlockAcquisitionState::Complete(..) => false,
        }
    }

    pub(super) fn register_deploy(
        &mut self,
        deploy_id: DeployId,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        let was_waiting_for_deploys = self.waiting_for_deploys();
        let acceptance = self
            .acquisition_state
            .register_deploy(deploy_id, self.should_fetch_execution_state);
        self.handle_acceptance(maybe_peer, acceptance, was_waiting_for_deploys)
    }

    pub(super) fn register_peers(&mut self, peers: Vec<NodeId>) {
        if peers.is_empty() {
            // We asked for peers but none were provided. Exit early without
            // clearing the latch so that we don't ask again needlessly.
            trace!("BlockSynchronizer: no peers available");
            return;
        }
        if !(self.is_finished() || self.is_failed()) {
            peers
                .into_iter()
                .for_each(|peer| self.peer_list.register_peer(peer));
        }
        self.touch();
    }

    fn handle_acceptance(
        &mut self,
        maybe_peer: Option<NodeId>,
        acceptance: Result<Option<Acceptance>, BlockAcquisitionError>,
        should_unlatch: bool,
    ) -> Result<(), Error> {
        match acceptance {
            Ok(Some(Acceptance::NeededIt)) => {
                // Got a useful response. Unlatch in all cases since we want to get the next item.
                self.touch();
                if let Some(peer) = maybe_peer {
                    self.promote_peer(peer);
                }
            }
            Ok(Some(Acceptance::HadIt)) => {
                // Already had this item, which means that this was a late response for a previous
                // fetch. We don't unlatch in this case and wait for a valid response.
            }
            Ok(None) => {
                if should_unlatch {
                    self.latch_decrement();
                }
            }
            Err(error) => {
                if let Some(peer) = maybe_peer {
                    self.disqualify_peer(peer);
                }

                // If we were waiting for a response and the item was not good,
                // decrement latch. Fetch will be retried when unlatched.
                if should_unlatch {
                    self.latch_decrement();
                }

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
        self.latch.unlatch();
    }

    pub(crate) fn peer_list(&self) -> &PeerList {
        &self.peer_list
    }
}
