use std::fmt::{Display, Formatter};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use tracing::{error, warn};

use crate::components::block_synchronizer::{
    block_acquisition,
    block_acquisition::{BlockAcquisitionAction, BlockAcquisitionState},
};
use casper_hashing::Digest;
use casper_types::{EraId, Timestamp};

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::DeployAcquisition,
        execution_results_acquisition::{ExecutionResultsAcquisition, ExecutionResultsRootHash},
        need_next::NeedNext,
        peer_list::PeerList,
        signature_acquisition::SignatureAcquisition,
    },
    types::{
        BlockAdded, BlockExecutionResultsOrChunk, BlockHash, BlockHeader, DeployHash,
        EraValidatorWeights, FinalitySignature, NodeId, SyncLeap,
    },
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
    last_progress: Option<Timestamp>,

    // acquired state
    acquisition_state: BlockAcquisitionState,
}

impl BlockBuilder {
    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn new(
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
            last_progress: None,
        }
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn new_from_sync_leap(
        sync_leap: SyncLeap,
        validator_weights: EraValidatorWeights,
        peers: Vec<NodeId>,
        fault_tolerance_fraction: Ratio<u64>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) -> Self {
        if let Some(fat_block_header) = sync_leap.highest_block_header() {
            let block_header = fat_block_header.block_header;
            let block_hash = block_header.hash();
            let sigs = fat_block_header.block_signatures;
            let era_id = Some(block_header.era_id());
            let public_keys = sigs.proofs.into_iter().map(|(k, _)| k).collect_vec();
            let signature_acquisition = SignatureAcquisition::new(public_keys);
            let acquisition_state = BlockAcquisitionState::HaveBlockHeader(
                Box::new(block_header),
                signature_acquisition,
            );
            let mut peer_list = PeerList::new(max_simultaneous_peers);
            peers.iter().for_each(|p| peer_list.register_peer(*p));

            BlockBuilder {
                block_hash,
                era_id,
                validator_weights: Some(validator_weights),
                acquisition_state,
                peer_list,
                should_fetch_execution_state,
                started: None,
                last_progress: None,
            }
        } else {
            let block_hash = sync_leap.trusted_block_header.hash();
            BlockBuilder::new(
                block_hash,
                should_fetch_execution_state,
                max_simultaneous_peers,
            )
        }
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn needs_validators(&self, era_id: EraId) -> bool {
        match self.validator_weights {
            None => match self.era_id {
                None => false, // can't get validators w/o era, so must be false
                Some(e) => e == era_id,
            },
            Some(_) => false,
        }
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn abort(&mut self) -> bool {
        self.acquisition_state = BlockAcquisitionState::Fatal;
        self.flush_peers();
        self.touch();
        self.is_fatal()
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn block_height(&self) -> Option<u64> {
        self.acquisition_state.block_height()
    }

    // NOT WIRED
    pub(crate) fn builder_state(&self) -> &BlockAcquisitionState {
        &self.acquisition_state
    }

    // NOT WIRED
    pub(crate) fn started(&self) -> Option<Timestamp> {
        self.started
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn last_progress_time(&self) -> Option<Timestamp> {
        self.last_progress
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn is_fatal(&self) -> bool {
        self.acquisition_state == BlockAcquisitionState::Fatal
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn is_complete(&self) -> bool {
        matches!(
            self.acquisition_state,
            BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
        )
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_peer(&mut self, peer: NodeId) {
        if self.is_complete() || self.is_fatal() {
            return;
        }
        self.peer_list.register_peer(peer);
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn dishonest_peers(&self) -> Vec<NodeId> {
        self.peer_list.dishonest_peers()
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn disqualify_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.disqualify_peer(peer);
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn promote_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.promote_peer(peer);
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn demote_peer(&mut self, peer: Option<NodeId>) {
        self.peer_list.demote_peer(peer);
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn flush_dishonest_peers(&mut self) {
        self.peer_list.flush_dishonest_peers();
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn block_acquisition_action(&mut self, rng: &mut NodeRng) -> BlockAcquisitionAction {
        if self.peer_list.need_peers() {
            return BlockAcquisitionAction::peers(self.block_hash);
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

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_block_header(
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

    // NOT WIRED
    pub(crate) fn register_block_added(
        &mut self,
        block_added: &BlockAdded,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .with_body(block_added.block(), self.should_fetch_execution_state)
        {
            self.disqualify_peer(maybe_peer);
            return Err(Error::BlockAcquisition(error));
        }
        self.touch();
        self.promote_peer(maybe_peer);
        Ok(())
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        maybe_peer: Option<NodeId>,
    ) -> Result<(), Error> {
        if let Some(validator_weights) = self.validator_weights.to_owned() {
            match self.acquisition_state.with_signature(
                finality_signature,
                validator_weights,
                self.should_fetch_execution_state,
            ) {
                Err(error) => {
                    self.disqualify_peer(maybe_peer);
                    return Err(Error::BlockAcquisition(error));
                }
                Ok(true) => {
                    self.touch();
                    self.promote_peer(maybe_peer);
                }
                Ok(false) => {
                    warn!(
                        ?maybe_peer,
                        "received a finality signature that we already have"
                    )
                }
            }
            Ok(())
        } else {
            Err(Error::MissingValidatorWeights(self.block_hash))
        }
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_global_state(&mut self, global_state: Digest) -> Result<(), Error> {
        if let Err(error) = self
            .acquisition_state
            .with_global_state(global_state, self.should_fetch_execution_state)
        {
            return Err(Error::BlockAcquisition(error));
        }
        self.touch();
        Ok(())
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_execution_results_root_hash(
        &mut self,
        execution_results_root_hash: ExecutionResultsRootHash,
    ) -> Result<(), Error> {
        if let Err(err) = self.acquisition_state.with_execution_results_root_hash(
            execution_results_root_hash,
            self.should_fetch_execution_state,
        ) {
            return Err(Error::BlockAcquisition(err));
        }
        self.touch();
        Ok(())
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_deploy(
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

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_fetched_execution_results(
        &mut self,
        maybe_peer: Option<NodeId>,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
    ) -> Result<Option<Vec<casper_types::ExecutionResult>>, Error> {
        match self.acquisition_state.with_execution_results_or_chunk(
            block_execution_results_or_chunk,
            self.should_fetch_execution_state,
        ) {
            Ok(maybe_results) => {
                self.touch();
                self.promote_peer(maybe_peer);
                Ok(maybe_results)
            }
            Err(error) => {
                self.disqualify_peer(maybe_peer);
                Err(Error::BlockAcquisition(error))
            }
        }
    }

    // WIRED IN BLOCK SYNCHRONIZER
    pub(crate) fn register_stored_execution_results(&mut self) -> Result<(), Error> {
        if let Err(err) = self
            .acquisition_state
            .with_stored_execution_results(self.should_fetch_execution_state)
        {
            return Err(Error::BlockAcquisition(err));
        }
        self.touch();
        Ok(())
    }

    // NOT WIRED
    pub(crate) fn register_era_validator_weights(
        &mut self,
        validator_weights: EraValidatorWeights,
    ) {
        self.validator_weights = Some(validator_weights);
        self.touch();
    }

    // NOT WIRED
    pub(crate) fn register_peers(&mut self, peers: Vec<NodeId>) -> Result<(), Error> {
        peers
            .into_iter()
            .for_each(|peer| self.peer_list.register_peer(peer));
        self.touch();
        Ok(())
    }

    fn flush_peers(&mut self) {
        self.peer_list.flush();
    }

    fn touch(&mut self) {
        let now = Timestamp::now();
        if self.started.is_none() {
            self.started = Some(now);
        }
        self.last_progress = Some(now);
    }
}
