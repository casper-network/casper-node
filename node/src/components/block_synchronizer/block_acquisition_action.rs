use std::fmt::{self, Display, Formatter};
use tracing::{debug, info, warn};

use casper_hashing::Digest;
use casper_types::{system::auction::ValidatorWeights, EraId, ProtocolVersion, PublicKey};

use crate::{
    components::block_synchronizer::{
        deploy_acquisition::DeployIdentifier, need_next::NeedNext, peer_list::PeerList,
        signature_acquisition::SignatureAcquisition, BlockAcquisitionError,
        ExecutionResultsAcquisition, ExecutionResultsChecksum,
    },
    types::{
        chainspec::LegacyRequiredFinality, Block, BlockExecutionResultsOrChunkId, BlockHash,
        BlockHeader, DeployHash, DeployId, EraValidatorWeights, NodeId, SignatureWeight,
    },
    NodeRng,
};

use super::global_state_acquisition::GlobalStateAcquisition;

use super::block_acquisition::signatures_from_missing_validators;

#[derive(Debug)]
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

    pub(super) fn get_block_header_from_storage(
        needed_for_block: BlockHash,
        block_hash: BlockHash,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: Vec::new(),
            need_next: NeedNext::BlockHeaderFromStorage(needed_for_block, block_hash),
        }
    }

    pub(super) fn era_validators_from_contract_runtime(
        request_data: Vec<(Digest, ProtocolVersion)>,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: Vec::new(),
            need_next: NeedNext::EraValidatorsFromContractRuntime(request_data),
        }
    }

    pub(super) fn update_era_validators(
        era_id: EraId,
        validator_weights: ValidatorWeights,
    ) -> Self {
        BlockAcquisitionAction {
            peers_to_ask: Vec::new(),
            need_next: NeedNext::UpdateEraValidators(era_id, validator_weights),
        }
    }

    pub(super) fn global_state(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_hash: BlockHash,
        state_root_hash: Digest,
        global_state_acquisition: &mut GlobalStateAcquisition,
        max_parallel_trie_fetches: usize,
    ) -> Self {
        let peers_to_ask = peer_list.qualified_peers(rng);
        let tries_to_store = global_state_acquisition.tries_to_store();
        let tries_to_fetch = global_state_acquisition.tries_to_fetch(max_parallel_trie_fetches);
        BlockAcquisitionAction {
            peers_to_ask,
            need_next: NeedNext::GlobalState(
                block_hash,
                state_root_hash,
                tries_to_store,
                tries_to_fetch,
            ),
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn strict_finality_signatures(
        peer_list: &PeerList,
        rng: &mut NodeRng,
        block_header: &BlockHeader,
        validator_weights: &EraValidatorWeights,
        signature_acquisition: &mut SignatureAcquisition,
        is_historical: bool,
        legacy_required_finality: LegacyRequiredFinality,
        max_simultaneous_peers: usize,
    ) -> Self {
        let block_hash = block_header.block_hash();
        let validator_keys = signature_acquisition.have_signatures();
        let signature_weight = validator_weights.signature_weight(validator_keys);

        if enough_signatures(
            signature_weight,
            legacy_required_finality,
            signature_acquisition.is_checkable(),
            is_historical,
        ) {
            if is_historical {
                // we have enough signatures; need to make sure we've stored the necessary bits
                BlockAcquisitionAction {
                    peers_to_ask: vec![],
                    need_next: NeedNext::BlockMarkedComplete(block_hash, block_header.height()),
                }
            } else {
                BlockAcquisitionAction {
                    peers_to_ask: vec![],
                    need_next: NeedNext::EnqueueForExecution(block_hash, block_header.height()),
                }
            }
        } else {
            // Collect signatures with Vacant state or which are currently missing from the
            // SignatureAcquisition.
            signatures_from_missing_validators(
                validator_weights,
                signature_acquisition,
                max_simultaneous_peers,
                peer_list,
                rng,
                block_header,
            )
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
        block_header: &BlockHeader,
        peer_list: &PeerList,
        rng: &mut NodeRng,
        validator_weights: &EraValidatorWeights,
        signatures: &mut SignatureAcquisition,
        needs_deploy: Option<DeployIdentifier>,
        is_historical: bool,
        legacy_required_finality: LegacyRequiredFinality,
        max_simultaneous_peers: usize,
    ) -> Self {
        match needs_deploy {
            Some(DeployIdentifier::ById(deploy_id)) => {
                info!("BlockAcquisition: requesting missing deploy by ID");
                BlockAcquisitionAction::deploy_by_id(
                    block_header.block_hash(),
                    deploy_id,
                    peer_list,
                    rng,
                )
            }
            Some(DeployIdentifier::ByHash(deploy_hash)) => {
                info!("BlockAcquisition: requesting missing deploy by hash");
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
                is_historical,
                legacy_required_finality,
                max_simultaneous_peers,
            ),
        }
    }
}

fn enough_signatures(
    signature_weight: SignatureWeight,
    legacy_required_finality: LegacyRequiredFinality,
    is_checkable: bool,
    is_historical: bool,
) -> bool {
    if is_checkable {
        // Normal blocks:
        signature_weight.is_sufficient(false == is_historical)
    } else {
        // Legacy blocks:
        matches!((legacy_required_finality, signature_weight), |(
            LegacyRequiredFinality::Any,
            SignatureWeight::Insufficient | SignatureWeight::Weak | SignatureWeight::Strict,
        )| (
            LegacyRequiredFinality::Weak,
            SignatureWeight::Weak | SignatureWeight::Strict
        )
            | (LegacyRequiredFinality::Strict, SignatureWeight::Strict))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enough_signatures_params_insufficient_any_false_false() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Any,
            false,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_weak_any_false_false() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Any,
            false,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_any_false_false() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Any,
            false,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_weak_false_false() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Weak,
            false,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_weak_false_false() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Weak,
            false,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_weak_false_false() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Weak,
            false,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_strict_false_false() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Strict,
            false,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_strict_false_false() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Strict,
            false,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_strict_strict_false_false() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Strict,
            false,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_any_true_false() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Any,
            true,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_any_true_false() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Any,
            true,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_strict_any_true_false() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Any,
            true,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_weak_true_false() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Weak,
            true,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_weak_true_false() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Weak,
            true,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_strict_weak_true_false() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Weak,
            true,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_strict_true_false() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Strict,
            true,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_strict_true_false() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Strict,
            true,
            false,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_strict_strict_true_false() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Strict,
            true,
            false,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_any_false_true() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Any,
            false,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_weak_any_false_true() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Any,
            false,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_any_false_true() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Any,
            false,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_weak_false_true() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Weak,
            false,
            true,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_weak_false_true() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Weak,
            false,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_weak_false_true() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Weak,
            false,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_strict_false_true() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Strict,
            false,
            true,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_strict_false_true() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Strict,
            false,
            true,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_strict_strict_false_true() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Strict,
            false,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_any_true_true() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Any,
            true,
            true,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_any_true_true() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Any,
            true,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_any_true_true() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Any,
            true,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_weak_true_true() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Weak,
            true,
            true,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_weak_true_true() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Weak,
            true,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_weak_true_true() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Weak,
            true,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_insufficient_strict_true_true() {
        let result = enough_signatures(
            SignatureWeight::Insufficient,
            LegacyRequiredFinality::Strict,
            true,
            true,
        );

        assert!(result == false);
    }

    #[test]
    fn enough_signatures_params_weak_strict_true_true() {
        let result = enough_signatures(
            SignatureWeight::Weak,
            LegacyRequiredFinality::Strict,
            true,
            true,
        );

        assert!(result == true);
    }

    #[test]
    fn enough_signatures_params_strict_strict_true_true() {
        let result = enough_signatures(
            SignatureWeight::Strict,
            LegacyRequiredFinality::Strict,
            true,
            true,
        );

        assert!(result == true);
    }
}
