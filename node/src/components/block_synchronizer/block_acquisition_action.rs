use std::fmt::{self, Display, Formatter};
use tracing::{debug, warn};

use casper_types::{Block, BlockHash, DeployHash, DeployId, Digest, EraId, PublicKey};

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::DeployIdentifier, need_next::NeedNext, peer_list::PeerList,
        signature_acquisition::SignatureAcquisition, BlockAcquisitionError,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{BlockExecutionResultsOrChunkId, EraValidatorWeights, ExecutableBlock, NodeId},
    NodeRng,
};

use super::block_acquisition::signatures_from_missing_validators;

#[derive(Debug, PartialEq)]
pub(crate) struct BlockAcquisitionAction {
    peers_to_ask: Vec<NodeId>,
    need_next: NeedNext,
}

impl Display for BlockAcquisitionAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} from {} peers",
            self.need_next,
            self.peers_to_ask.len()
        )
    }
}

impl BlockAcquisitionAction {
    pub(super) fn need_next(&self) -> NeedNext {
        self.need_next.clone()
    }

    pub(super) fn peers_to_ask(&self) -> Vec<NodeId> {
        self.peers_to_ask.to_vec()
    }

    pub(super) fn need_nothing(block_hash: BlockHash) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Nothing(block_hash),
        }
    }

    pub(super) fn peers(block_hash: BlockHash) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::Peers(block_hash),
        }
    }

    pub(super) fn execution_results_checksum(
        block_hash: BlockHash,
        global_state_root_hash: Digest,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::ExecutionResultsChecksum(block_hash, global_state_root_hash),
        }
    }

    pub(super) fn execution_results(
        block_hash: BlockHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        next: BlockExecutionResultsOrChunkId,
        checksum: ExecutionResultsChecksum,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::ExecutionResults(block_hash, next, checksum),
        }
    }

    pub(super) fn approvals_hashes(block: &Block, peer_list: &PeerList, rng: &mut NodeRng) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::ApprovalsHashes(*block.hash(), Box::new(block.clone())),
        }
    }

    pub(super) fn deploy_by_hash(
        block_hash: BlockHash,
        deploy_hash: DeployHash,
        peer_list: &PeerList,
        rng: &mut NodeRng,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::DeployByHash(block_hash, deploy_hash),
        }
    }

    pub(super) fn deploy_by_id(
        block_hash: BlockHash,
        deploy_id: DeployId,
        peer_list: &PeerList,
        rng: &mut NodeRng,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::DeployById(block_hash, deploy_id),
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
        era_id: EraId,
        block_hash: BlockHash,
        missing_signatures: Vec<PublicKey>,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);

        debug!(
            %era_id,
            missing_signatures = missing_signatures.len(),
            peers_to_ask = peers_to_ask.len(),
            "BlockSynchronizer: requesting finality signatures");

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

    pub(super) fn switch_to_have_sufficient_finality(
        block_hash: BlockHash,
        block_height: u64,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::SwitchToHaveStrictFinality(block_hash, block_height),
        }
    }

    pub(super) fn block_marked_complete(block_hash: BlockHash, block_height: u64) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::BlockMarkedComplete(block_hash, block_height),
        }
    }

    pub(super) fn make_executable_block(block_hash: BlockHash, block_height: u64) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::MakeExecutableBlock(block_hash, block_height),
        }
    }

    pub(super) fn enqueue_block_for_execution(
        block_hash: &BlockHash,
        executable_block: Box<ExecutableBlock>,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: vec![],
            need_next: NeedNext::EnqueueForExecution(
                *block_hash,
                executable_block.height,
                executable_block,
            ),
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

    pub(super) fn era_validators(peer_list: &PeerList, rng: &mut NodeRng, era_id: EraId) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::EraValidators(era_id),
        }
    }

    pub(super) fn maybe_execution_results(
        block: &Block,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        exec_results: &mut ExecutionResultsAcquisition,
    ) -> Result<Self, BlockAcquisitionError> {
        match exec_results {
            ExecutionResultsAcquisition::Needed { .. } => {
                Ok(BlockAcquisitionAction::execution_results_checksum(
                    *block.hash(),
                    *block.state_root_hash(),
                ))
            }
            acq @ ExecutionResultsAcquisition::Pending { .. }
            | acq @ ExecutionResultsAcquisition::Acquiring { .. } => {
                match acq.needs_value_or_chunk() {
                    None => {
                        warn!(
                            block_hash = %block.hash(),
                            "execution_results_acquisition.needs_value_or_chunk() should never be \
                            None for these variants"
                        );
                        Err(BlockAcquisitionError::InvalidAttemptToAcquireExecutionResults)
                    }
                    Some((next, checksum)) => Ok(BlockAcquisitionAction::execution_results(
                        *block.hash(),
                        peer_list,
                        rng,
                        next,
                        checksum,
                    )),
                }
            }
            ExecutionResultsAcquisition::Complete { .. } => Ok(
                BlockAcquisitionAction::approvals_hashes(block, peer_list, rng),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn maybe_needs_deploy(
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        validator_weights: &EraValidatorWeights,
        signatures: &mut SignatureAcquisition,
        needs_deploy: Option<DeployIdentifier>,
        is_historical: bool,
        max_simultaneous_peers: u8,
    ) -> Self {
        match needs_deploy {
            Some(DeployIdentifier::ById(deploy_id)) => {
                debug!("BlockAcquisition: requesting missing deploy by ID");
                BlockAcquisitionAction::deploy_by_id(block_hash, deploy_id, peer_list, rng)
            }
            Some(DeployIdentifier::ByHash(deploy_hash)) => {
                debug!("BlockAcquisition: requesting missing deploy by hash");
                BlockAcquisitionAction::deploy_by_hash(block_hash, deploy_hash, peer_list, rng)
            }
            None => {
                if signatures.has_sufficient_finality(is_historical, true) {
                    BlockAcquisitionAction::switch_to_have_sufficient_finality(
                        block_hash,
                        block_height,
                    )
                } else {
                    signatures_from_missing_validators(
                        validator_weights,
                        signatures,
                        max_simultaneous_peers,
                        peer_list,
                        rng,
                        era_id,
                        block_hash,
                    )
                }
            }
        }
    }
}
