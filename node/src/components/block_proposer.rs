//! Block proposer.
//!
//! The block proposer stores deploy hashes in memory, tracking their suitability for inclusion into
//! a new block. Upon request, it returns a list of candidates that can be included.

mod cached_state;
mod config;
mod deploy_sets;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    convert::Infallible,
    sync::Arc,
    time::Duration,
};

use datasize::DataSize;
use futures::join;
use prometheus::{self, Registry};
use tracing::{debug, error, info, warn};

use casper_types::{PublicKey, Timestamp};

use crate::{
    components::{
        consensus::{BlockContext, ClContext},
        Component,
    },
    effect::{
        announcements::BlockProposerAnnouncement,
        requests::{BlockPayloadRequest, BlockProposerRequest, StateStoreRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{
        appendable_block::{AddError, AppendableBlock},
        chainspec::DeployConfig,
        Approval, BlockPayload, Chainspec, DeployHash, DeployHeader, DeployOrTransferHash,
        DeployWithApprovals, FinalizedBlock,
    },
    NodeRng,
};
use cached_state::CachedState;
pub use config::Config;
use deploy_sets::{BlockProposerDeploySets, PendingDeployInfo, PruneResult};
pub(crate) use event::{DeployInfo, Event};
use metrics::Metrics;

/// Block proposer component.
#[derive(DataSize, Debug)]
pub(crate) struct BlockProposer {
    /// The current state of the proposer component.
    state: BlockProposerState,

    /// Metrics, present in all states.
    metrics: Metrics,
}

const STATE_KEY: &[u8] = b"block proposer";

/// Interval after which a pruning of the internal sets is triggered.
// TODO: Make configurable.
const PRUNE_INTERVAL: Duration = Duration::from_secs(10);

/// Experimentally, deploys are in the range of 270-280 bytes, we use this to determine if we are
/// within a threshold to break iteration of `pending` early.
const DEPLOY_APPROX_MIN_SIZE: usize = 300;

/// The type of values expressing the block height in the chain.
type BlockHeight = u64;

/// A queue of blocks that we know have been finalized but are still missing some ancestors.
type FinalizationQueue = HashMap<BlockHeight, FinalizedBlock>;

/// A queue of requests we can't respond to yet, because we aren't up to date on finalized blocks.
/// The key is the height of the next block we will expect to be finalized at the point when we can
/// fulfill the corresponding requests.
type RequestQueue = HashMap<BlockHeight, Vec<BlockPayloadRequest>>;

/// Current operational state of a block proposer.
#[derive(DataSize, Debug)]
#[allow(clippy::large_enum_variant)]
enum BlockProposerState {
    /// Block proposer is initializing, waiting for a state snapshot.
    Initializing {
        /// Events cached pending transition to `Ready` state when they can be handled.
        pending: Vec<Event>,
        /// The deploy config from the current chainspec.
        deploy_config: DeployConfig,
        /// The configuration, containing local settings for deploy selection.
        local_config: Config,
    },
    /// Normal operation.
    Ready(BlockProposerReady),
}

impl BlockProposer {
    /// Creates a new block proposer instance.
    pub(crate) fn new<REv>(
        registry: Registry,
        effect_builder: EffectBuilder<REv>,
        next_finalized_block: BlockHeight,
        chainspec: &Chainspec,
        local_config: Config,
    ) -> Result<(Self, Effects<Event>), prometheus::Error>
    where
        REv: From<Event> + From<StorageRequest> + From<StateStoreRequest> + Send + 'static,
    {
        debug!(%next_finalized_block, "creating block proposer");
        let max_ttl = chainspec.deploy_config.max_ttl;
        let effects = async move {
            join!(
                effect_builder.get_finalized_blocks(max_ttl),
                effect_builder.load_state::<CachedState>(STATE_KEY.into())
            )
        }
        .event(
            move |(finalized_blocks, maybe_cached_state)| Event::Loaded {
                finalized_blocks,
                next_finalized_block,
                cached_state: maybe_cached_state.unwrap_or_default(),
            },
        );

        let block_proposer = BlockProposer {
            state: BlockProposerState::Initializing {
                pending: Vec::new(),
                deploy_config: chainspec.deploy_config,
                local_config,
            },
            metrics: Metrics::new(registry)?,
        };

        Ok((block_proposer, effects))
    }
}

impl<REv> Component<REv> for BlockProposer
where
    REv: From<Event>
        + From<StorageRequest>
        + From<StateStoreRequest>
        + From<BlockProposerAnnouncement>
        + Send
        + 'static,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let mut effects = Effects::new();

        // We handle two different states in the block proposer, but our "ready" state is
        // encapsulated in a separate type to simplify the code. The `Initializing` state is simple
        // enough to handle it here directly.
        match (&mut self.state, event) {
            (
                BlockProposerState::Initializing {
                    ref mut pending,
                    deploy_config,
                    local_config,
                },
                Event::Loaded {
                    finalized_blocks,
                    next_finalized_block,
                    cached_state,
                },
            ) => {
                let (sets, pruned_hashes) = BlockProposerDeploySets::new(
                    finalized_blocks,
                    next_finalized_block,
                    cached_state,
                    deploy_config.max_ttl,
                );

                let mut new_ready_state = BlockProposerReady {
                    sets,
                    deploy_config: *deploy_config,
                    request_queue: Default::default(),
                    local_config: local_config.clone(),
                };

                // Announce pruned hashes.
                let pruned_count = pruned_hashes.total_pruned;
                debug!(%pruned_count, "pruned deploys from buffer on loading");
                effects.extend(
                    effect_builder
                        .announce_expired_deploys(pruned_hashes.expired_hashes_to_be_announced)
                        .ignore(),
                );

                // After pruning, we store a state snapshot.
                effects.extend(
                    effect_builder
                        .save_state(STATE_KEY.into(), CachedState::from(&new_ready_state.sets))
                        .ignore(),
                );

                // Replay postponed events onto new state.
                for ev in pending.drain(..) {
                    effects.extend(new_ready_state.handle_event(effect_builder, ev));
                }

                self.state = BlockProposerState::Ready(new_ready_state);

                // Start pruning deploys after a delay.
                effects.extend(
                    effect_builder
                        .set_timeout(PRUNE_INTERVAL)
                        .event(|_| Event::Prune),
                );
            }
            (
                BlockProposerState::Initializing {
                    ref mut pending, ..
                },
                event,
            ) => {
                // Any incoming events are just buffered until initialization is complete.
                pending.push(event);
            }

            (BlockProposerState::Ready(ref mut ready_state), event) => {
                effects.extend(ready_state.handle_event(effect_builder, event));

                // Update metrics after the effects have been applied.
                self.metrics.pending_deploys.set(
                    ready_state.sets.pending_deploys.len() as i64
                        + ready_state.sets.pending_transfers.len() as i64,
                );
            }
        };

        effects
    }
}

/// State of operational block proposer.
#[derive(DataSize, Debug)]
#[cfg_attr(test, derive(Default))]
struct BlockProposerReady {
    /// Set of deploys currently stored in the block proposer.
    sets: BlockProposerDeploySets,
    /// We don't need the whole Chainspec here, just the deploy config.
    deploy_config: DeployConfig,
    /// The queue of requests awaiting being handled.
    request_queue: RequestQueue,
    /// The block proposer configuration, containing local settings for selecting deploys.
    local_config: Config,
}

impl BlockProposerReady {
    fn handle_event<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event: Event,
    ) -> Effects<Event>
    where
        REv: Send + From<StateStoreRequest> + From<BlockProposerAnnouncement>,
    {
        match event {
            Event::Request(BlockProposerRequest::RequestBlockPayload(request)) => {
                if request.next_finalized > self.sets.next_finalized {
                    warn!(
                        %request.next_finalized, %self.sets.next_finalized,
                        "received request before finalization announcement"
                    );
                    self.request_queue
                        .entry(request.next_finalized)
                        .or_default()
                        .push(request);
                    Effects::new()
                } else {
                    info!(%request.next_finalized, "proposing a block payload");
                    request
                        .responder
                        .respond(self.propose_block_payload(
                            self.deploy_config,
                            request.context,
                            request.accusations,
                            request.random_bit,
                        ))
                        .ignore()
                }
            }
            Event::BufferDeploy {
                hash,
                approvals,
                deploy_info,
            } => {
                self.add_deploy(Timestamp::now(), hash, approvals, *deploy_info);
                Effects::new()
            }
            Event::Prune => {
                // Re-trigger timer after `PRUNE_INTERVAL`.
                let mut effects = effect_builder
                    .set_timeout(PRUNE_INTERVAL)
                    .event(|_| Event::Prune);

                // Announce pruned hashes.
                let pruned_hashes = self.prune(Timestamp::now());
                let pruned_count = pruned_hashes.total_pruned;
                debug!(%pruned_count, "pruned deploys from buffer");
                effects.extend(
                    effect_builder
                        .announce_expired_deploys(pruned_hashes.expired_hashes_to_be_announced)
                        .ignore(),
                );

                // After pruning, we store a state snapshot.
                effects.extend(
                    effect_builder
                        .save_state(STATE_KEY.into(), CachedState::from(&self.sets))
                        .ignore(),
                );

                effects
            }
            Event::Loaded { .. } => {
                // This should never happen, but we can just ignore the event and carry on.
                error!("got loaded event for block proposer state during ready state");
                Effects::new()
            }
            Event::FinalizedBlock(block) => {
                let mut height = block.height();
                if height > self.sets.next_finalized {
                    warn!(
                        %height, next_finalized = %self.sets.next_finalized,
                        "received finalized blocks out of order; queueing"
                    );
                    // safe to subtract 1 - height will never be 0 in this branch, because
                    // next_finalized is at least 0, and height has to be greater
                    self.sets.finalization_queue.insert(height - 1, *block);
                    Effects::new()
                } else {
                    debug!(%height, "handling finalized block");
                    let mut effects = self.handle_finalized_block(&*block);
                    while let Some(block) = self.sets.finalization_queue.remove(&height) {
                        info!(%height, "removed finalization queue entry");
                        height += 1;
                        effects.extend(self.handle_finalized_block(&block));
                    }
                    effects
                }
            }
        }
    }

    /// Adds a deploy or a transfer to the block proposer.
    fn add_deploy(
        &mut self,
        current_instant: Timestamp,
        hash: DeployOrTransferHash,
        approvals: BTreeSet<Approval>,
        deploy_info: DeployInfo,
    ) {
        if hash.is_transfer() {
            // only add the transfer if it isn't contained in a finalized block
            if self
                .sets
                .finalized_transfers
                .contains_key(hash.deploy_hash())
            {
                info!(%hash, "finalized transfer rejected from the buffer");
                self.sets
                    .add_finalized_transfer(*hash.deploy_hash(), deploy_info.header.expires());
                return;
            }
            self.sets.pending_transfers.insert(
                *hash.deploy_hash(),
                PendingDeployInfo {
                    approvals,
                    info: deploy_info,
                    timestamp: current_instant,
                },
            );
            info!(%hash, "added transfer to the buffer");
        } else {
            // only add the deploy if it isn't contained in a finalized block
            if self.sets.finalized_deploys.contains_key(hash.deploy_hash()) {
                info!(%hash, "finalized deploy rejected from the buffer");
                self.sets
                    .add_finalized_deploy(*hash.deploy_hash(), deploy_info.header.expires());
                return;
            }
            self.sets.pending_deploys.insert(
                *hash.deploy_hash(),
                PendingDeployInfo {
                    approvals,
                    info: deploy_info,
                    timestamp: current_instant,
                },
            );
            info!(%hash, "added deploy to the buffer");
        }
    }

    /// Handles finalization of a block.
    fn handle_finalized_block(&mut self, block: &FinalizedBlock) -> Effects<Event> {
        for deploy_hash in block.deploy_hashes() {
            let expiry = match self.sets.pending_deploys.remove(deploy_hash) {
                Some(pending_deploy_info) => pending_deploy_info.info.header.expires(),
                None => block.timestamp().saturating_add(self.deploy_config.max_ttl),
            };
            self.sets.add_finalized_deploy(*deploy_hash, expiry);
        }
        for transfer_hash in block.transfer_hashes() {
            let expiry = match self.sets.pending_transfers.remove(transfer_hash) {
                Some(pending_deploy_info) => pending_deploy_info.info.header.expires(),
                None => block.timestamp().saturating_add(self.deploy_config.max_ttl),
            };
            self.sets.add_finalized_transfer(*transfer_hash, expiry);
        }

        self.sets.next_finalized = self.sets.next_finalized.max(block.height() + 1);
        if let Some(requests) = self.request_queue.remove(&self.sets.next_finalized) {
            info!(height = %self.sets.next_finalized, "handling queued requests");
            requests
                .into_iter()
                .flat_map(|request| {
                    request
                        .responder
                        .respond(self.propose_block_payload(
                            self.deploy_config,
                            request.context,
                            request.accusations,
                            request.random_bit,
                        ))
                        .ignore()
                })
                .collect()
        } else {
            Effects::new()
        }
    }

    /// Checks if a deploy's dependencies are satisfied, so the deploy is eligible for inclusion.
    fn deps_resolved(&self, header: &DeployHeader, past_deploys: &HashSet<DeployHash>) -> bool {
        header
            .dependencies()
            .iter()
            .all(|dep| past_deploys.contains(dep) || self.contains_finalized(dep))
    }

    /// Returns a list of candidates for inclusion into a block.
    fn propose_block_payload(
        &mut self,
        deploy_config: DeployConfig,
        context: BlockContext<ClContext>,
        accusations: Vec<PublicKey>,
        random_bit: bool,
    ) -> Arc<BlockPayload> {
        let past_deploys = context
            .ancestor_values()
            .iter()
            .flat_map(|block_payload| block_payload.deploys_and_transfers_iter())
            .map(DeployOrTransferHash::into)
            .take_while(|hash| !self.contains_finalized(hash))
            .collect();
        let block_timestamp = context.timestamp();
        let mut appendable_block = AppendableBlock::new(deploy_config, block_timestamp);

        // We prioritize transfers over deploys, so we try to include them first.
        for (hash, pending_deploy_info) in &self.sets.pending_transfers {
            if !self.deps_resolved(&pending_deploy_info.info.header, &past_deploys)
                || past_deploys.contains(hash)
                || self.contains_finalized(hash)
                || block_timestamp.saturating_diff(pending_deploy_info.timestamp)
                    < self.local_config.deploy_delay
            {
                continue;
            }

            if let Err(err) = appendable_block.add_transfer(
                DeployWithApprovals::new(*hash, pending_deploy_info.approvals.clone()),
                &pending_deploy_info.info,
            ) {
                match err {
                    // We added the maximum number of transfers.
                    AddError::TransferCount | AddError::GasLimit | AddError::BlockSize => break,
                    // This transfer would exceed the approval count, but another one with fewer
                    // approvals might not.
                    AddError::ApprovalCount if pending_deploy_info.approvals.len() > 1 => (),
                    AddError::ApprovalCount => break,
                    // The deploy is not valid in this block, but might be valid in another.
                    AddError::InvalidDeploy => (),
                    // These errors should never happen when adding a transfer.
                    AddError::InvalidGasAmount | AddError::DeployCount | AddError::Duplicate => {
                        error!(?err, "unexpected error when adding transfer")
                    }
                }
            }
        }

        // Now we try to add other deploys to the block.
        for (hash, pending_deploy_info) in &self.sets.pending_deploys {
            if !self.deps_resolved(&pending_deploy_info.info.header, &past_deploys)
                || past_deploys.contains(hash)
                || self.contains_finalized(hash)
                || block_timestamp.saturating_diff(pending_deploy_info.timestamp)
                    < self.local_config.deploy_delay
            {
                continue;
            }

            if let Err(err) = appendable_block.add_deploy(
                DeployWithApprovals::new(*hash, pending_deploy_info.approvals.clone()),
                &pending_deploy_info.info,
            ) {
                match err {
                    // We added the maximum number of deploys.
                    AddError::DeployCount => break,
                    AddError::BlockSize => {
                        if appendable_block.total_size() + DEPLOY_APPROX_MIN_SIZE
                            > deploy_config.block_gas_limit as usize
                        {
                            break; // Probably no deploy will fit in this block anymore.
                        }
                    }
                    // This deploy would exceed the approval count, but another one with fewer
                    // approvals might not.
                    AddError::ApprovalCount if pending_deploy_info.approvals.len() > 1 => (),
                    AddError::ApprovalCount => break,
                    // The deploy is not valid in this block, but might be valid in another.
                    // TODO: Do something similar to DEPLOY_APPROX_MIN_SIZE for gas.
                    AddError::InvalidDeploy | AddError::GasLimit => (),
                    // These errors should never happen when adding a deploy.
                    AddError::TransferCount | AddError::Duplicate => {
                        error!(?err, "unexpected error when adding deploy")
                    }
                    AddError::InvalidGasAmount => {
                        error!("payment_amount couldn't be converted from motes to gas")
                    }
                }
            }
        }

        Arc::new(appendable_block.into_block_payload(accusations, random_bit))
    }

    /// Prunes expired deploy information from the BlockProposer, returns the hashes of deploys
    /// pruned.
    fn prune(&mut self, current_instant: Timestamp) -> PruneResult {
        self.sets.prune(current_instant)
    }

    fn contains_finalized(&self, hash: &DeployHash) -> bool {
        self.sets.finalized_deploys.contains_key(hash)
            || self.sets.finalized_transfers.contains_key(hash)
    }
}
