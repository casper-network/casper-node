use std::fmt::{self, Display, Formatter};
use tracing::{info, warn};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::components::block_synchronizer::deploy_acquisition::DeployIdentifier;
use crate::components::block_synchronizer::BlockAcquisitionError;
use crate::{
    components::block_synchronizer::{
        need_next::NeedNext, peer_list::PeerList, signature_acquisition::SignatureAcquisition,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{
        Block, BlockExecutionResultsOrChunkId, BlockHash, BlockHeader, DeployHash, DeployId,
        EraValidatorWeights, Item, NodeId, SignatureWeight,
    },
    NodeRng,
};

#[derive(Debug)]
pub(crate) struct BlockAcquisitionAction {
    peers_to_ask: Vec<NodeId>,
    need_next: NeedNext,
}

impl Display for BlockAcquisitionAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "need_next: {} from {} peers",
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
        block_header: &BlockHeader,
        missing_signatures: Vec<PublicKey>,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        let era_id = block_header.era_id();
        let block_hash = block_header.block_hash();
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::FinalitySignatures(block_hash, era_id, missing_signatures),
        }
    }

    pub(super) fn strict_finality_signatures(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_header: &BlockHeader,
        validator_weights: &EraValidatorWeights,
        signature_acquisition: &SignatureAcquisition,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        let era_id = block_header.era_id();
        let block_hash = block_header.block_hash();
        let block_height = block_header.height();

        if let SignatureWeight::Sufficient =
            validator_weights.has_sufficient_weight(signature_acquisition.have_signatures())
        {
            // we have enough signatures; need to make sure we've stored the necessary bits
            return BlockAcquisitionAction {
                peers_to_ask: vec![],
                need_next: NeedNext::BlockMarkedComplete(block_hash, block_height),
            };
        }
        // need more signatures
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::FinalitySignatures(
                block_hash,
                era_id,
                validator_weights
                    .missing_validators(signature_acquisition.have_signatures())
                    .cloned()
                    .collect(),
            ),
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

    pub(super) fn maybe_execution_results(
        block: &Block,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        exec_results: &mut ExecutionResultsAcquisition,
    ) -> Result<Self, BlockAcquisitionError> {
        match exec_results {
            ExecutionResultsAcquisition::Needed { .. } => {
                Ok(BlockAcquisitionAction::execution_results_checksum(
                    block.id(),
                    *block.state_root_hash(),
                ))
            }
            acq @ ExecutionResultsAcquisition::Pending { .. }
            | acq @ ExecutionResultsAcquisition::Acquiring { .. } => {
                match acq.needs_value_or_chunk() {
                    None => {
                        warn!(block_hash=%block.id(), "execution_results_acquisition.needs_value_or_chunk() should never be None for these variants");
                        Err(BlockAcquisitionError::InvalidAttemptToAcquireExecutionResults)
                    }
                    Some((next, checksum)) => Ok(BlockAcquisitionAction::execution_results(
                        block.id(),
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

    pub(super) fn maybe_needs_deploy(
        block_header: &BlockHeader,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        validator_weights: &EraValidatorWeights,
        signatures: &SignatureAcquisition,
        needs_deploy: Option<DeployIdentifier>,
    ) -> Self {
        match needs_deploy {
            Some(DeployIdentifier::ById(deploy_id)) => {
                info!("BlockAcquisition: requesting missing deploys by ID");
                BlockAcquisitionAction::deploy_by_id(
                    block_header.block_hash(),
                    deploy_id,
                    peer_list,
                    rng,
                )
            }
            Some(DeployIdentifier::ByHash(deploy_hash)) => {
                info!("BlockAcquisition: requesting missing deploys by hash");
                BlockAcquisitionAction::deploy_by_hash(
                    block_header.block_hash(),
                    deploy_hash,
                    peer_list,
                    rng,
                )
            }
            None => BlockAcquisitionAction::strict_finality_signatures(
                peer_list,
                rng,
                block_header,
                validator_weights,
                signatures,
            ),
        }
    }
}
