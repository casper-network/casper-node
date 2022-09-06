use std::collections::{BTreeMap, HashSet};

use datasize::DataSize;
use num_rational::Ratio;

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, Timestamp, U512};

use crate::types::{BlockHash, DeployHash, FinalitySignature, NodeId};

/// given a block hash we fetch
///     * block,
///     * finality signatures (all in one message)
///     * trie (if get all)
///     * deploys (one message per deploy)
///     * execution results (if get all)

#[derive(DataSize, Debug)]
pub(super) enum NeedNext {
    Block(BlockHash),
    FinalitySignatures(BlockHash),
    GlobalState(Digest),
    Deploy(DeployHash),
    ExecutionResults(DeployHash),
    Nothing,
}

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
enum DeployState {
    Vacant,
    HaveDeployBody,
    HaveDeployBodyWithEffects,
}

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum BlockAcquisitionState {
    Initialized,
    GettingBlock,
    GettingFinalitySignatures,
    GettingGlobalState,
    GettingDeploys,
    GettingExecutionResults,
    Complete,
}

#[derive(DataSize, Debug)]
pub(super) struct CompleteBlockBuilder {
    block_hash: BlockHash,
    builder_state: BlockAcquisitionState,
    state_root_hash: Option<Digest>,
    era_id: EraId,
    validators: BTreeMap<PublicKey, U512>,
    deploys: Option<BTreeMap<DeployHash, DeployState>>,
    finality_signatures: Option<BTreeMap<PublicKey, FinalitySignature>>,
    has_global_state: bool,
    peer_list: HashSet<NodeId>,
    should_fetch_execution_state: bool,
    started: Option<Timestamp>,
    last_progress_time: Option<Timestamp>,
}

impl CompleteBlockBuilder {
    pub(super) fn new(
        block_hash: BlockHash,
        era_id: EraId,
        validators: BTreeMap<PublicKey, U512>,
        get_everything: bool,
    ) -> Self {
        CompleteBlockBuilder {
            block_hash,
            builder_state: BlockAcquisitionState::Initialized,
            state_root_hash: None,
            era_id,
            validators,
            deploys: None,
            finality_signatures: None,
            has_global_state: false,
            peer_list: HashSet::new(),
            should_fetch_execution_state: get_everything,
            started: None,
            last_progress_time: None,
        }
    }

    pub(super) fn touch(&mut self) {
        let now = Timestamp::now();
        if self.started.is_none() {
            self.started = Some(now);
        }
        self.last_progress_time = Some(now);
    }

    pub(super) fn next_needed(&mut self, fault_tolerance_fraction: Ratio<u64>) -> NeedNext {
        if self.builder_state == BlockAcquisitionState::Complete {
            return NeedNext::Nothing;
        }

        self.touch();
        if self.has_block() == false {
            self.builder_state = BlockAcquisitionState::GettingBlock;
            return NeedNext::Block(self.block_hash);
        }

        if self.has_sufficient_weight(fault_tolerance_fraction, false) == false {
            self.builder_state = BlockAcquisitionState::GettingFinalitySignatures;
            return NeedNext::FinalitySignatures(self.block_hash);
        }

        if self.should_fetch_execution_state
            && self.has_global_state == false
            && self.state_root_hash.is_some()
        {
            self.builder_state = BlockAcquisitionState::GettingGlobalState;
            // Safe to unwrap as checked immediately above.
            return NeedNext::GlobalState(
                self.state_root_hash.expect("should have state root hash"),
            );
        }

        if let Some(deploy_hash) = self.next_deploy() {
            self.builder_state = BlockAcquisitionState::GettingDeploys;
            return NeedNext::Deploy(deploy_hash);
        }

        if self.should_fetch_execution_state {
            self.builder_state = BlockAcquisitionState::GettingExecutionResults;
            if let Some(deploy_hash) = self.next_execution_results() {
                return NeedNext::ExecutionResults(deploy_hash);
            }
        }

        self.builder_state = BlockAcquisitionState::Complete;
        NeedNext::Nothing
    }

    pub(super) fn is_syncing_global_state(&self) -> bool {
        self.state_root_hash.is_some() && self.has_global_state == false
    }

    fn next_deploy(&self) -> Option<DeployHash> {
        match self.deploys.as_ref() {
            None => None,
            Some(deploys) => deploys
                .iter()
                .filter_map(|(deploy_hash, deploy_fetch_state)| {
                    if *deploy_fetch_state == DeployState::Vacant {
                        Some(deploy_hash)
                    } else {
                        None
                    }
                })
                .copied()
                .next(),
        }
    }

    fn next_execution_results(&self) -> Option<DeployHash> {
        match self.deploys.as_ref() {
            None => None,
            Some(deploys) => deploys
                .iter()
                .filter_map(|(deploy_hash, deploy_fetch_state)| {
                    if *deploy_fetch_state == DeployState::HaveDeployBody {
                        Some(deploy_hash)
                    } else {
                        None
                    }
                })
                .copied()
                .next(),
        }
    }

    fn has_all_deploy_bodies(&self) -> bool {
        match self.deploys.as_ref() {
            Some(deploys) => {
                for deploy_state in deploys.values() {
                    match deploy_state {
                        DeployState::Vacant => return false,
                        DeployState::HaveDeployBody if self.should_fetch_execution_state => {
                            return false
                        }
                        DeployState::HaveDeployBody => (),
                        DeployState::HaveDeployBodyWithEffects => (),
                    }
                }
                true
            }
            None => false,
        }
    }

    pub(super) fn has_block(&self) -> bool {
        self.state_root_hash.is_some()
    }

    fn current_weight(&self) -> Option<U512> {
        match self.finality_signatures.as_ref() {
            None => None,
            Some(sigs) => Some(
                sigs.values()
                    .flat_map(|finality_signature| {
                        self.validators.get(&finality_signature.public_key).copied()
                    })
                    .sum(),
            ),
        }
    }

    pub(super) fn has_sufficient_weight(
        &self,
        fault_tolerance_fraction: Ratio<u64>,
        strict: bool,
    ) -> bool {
        let signature_weight = match self.current_weight() {
            None => return false,
            Some(weight) => weight,
        };
        let total_weight: U512 = self.validators.values().copied().sum();
        let threshold = if strict {
            Ratio::new(1, 2) * (Ratio::from_integer(1) + fault_tolerance_fraction)
        } else {
            fault_tolerance_fraction
        };
        signature_weight * U512::from(*threshold.denom())
            >= total_weight * U512::from(*threshold.numer())
    }

    pub(super) fn builder_state(&self) -> BlockAcquisitionState {
        self.builder_state
    }

    pub(super) fn is_complete(&self) -> bool {
        self.builder_state == BlockAcquisitionState::Complete
    }

    pub(super) fn is_initialized(&self) -> bool {
        self.builder_state == BlockAcquisitionState::Initialized
    }

    pub(super) fn register_peer(&mut self, peer: NodeId) -> bool {
        self.peer_list.insert(peer)
    }

    pub(super) fn started(&self) -> Option<Timestamp> {
        self.started
    }

    pub(super) fn last_progress_time(&self) -> Option<Timestamp> {
        self.last_progress_time
    }

    pub(super) fn peer_list(&self) -> &HashSet<NodeId> {
        &self.peer_list
    }
}
