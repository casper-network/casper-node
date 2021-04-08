//! Block proposer.
//!
//! The block proposer stores deploy hashes in memory, tracking their suitability for inclusion into
//! a new block. Upon request, it returns a list of candidates that can be included.

mod config;
mod deploy_sets;
mod event;
mod metrics;

#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    time::Duration,
};

pub use config::Config;
use datasize::DataSize;
use prometheus::{self, Registry};
use tracing::{debug, error, info, trace};

use crate::{
    components::Component,
    effect::{
        requests::{BlockProposerRequest, ProtoBlockRequest, StateStoreRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{
        appendable_block::{AddError, AppendableBlock},
        chainspec::DeployConfig,
        Chainspec, DeployHash, DeployHeader, ProtoBlock, Timestamp,
    },
    NodeRng,
};
pub(crate) use deploy_sets::BlockProposerDeploySets;
pub(crate) use event::{DeployType, Event};
use metrics::BlockProposerMetrics;

/// Block proposer component.
#[derive(DataSize, Debug)]
pub(crate) struct BlockProposer {
    /// The current state of the proposer component.
    state: BlockProposerState,

    /// Metrics, present in all states.
    metrics: BlockProposerMetrics,
}

/// Interval after which a pruning of the internal sets is triggered.
// TODO: Make configurable.
const PRUNE_INTERVAL: Duration = Duration::from_secs(10);

/// Experimentally, deploys are in the range of 270-280 bytes, we use this to determine if we are
/// within a threshold to break iteration of `pending` early.
const DEPLOY_APPROX_MIN_SIZE: usize = 300;

/// The type of values expressing the block height in the chain.
type BlockHeight = u64;

/// A queue of contents of blocks that we know have been finalized, but we are still missing
/// notifications about finalization of some of their ancestors. It maps block height to the
/// deploys contained in the corresponding block.
type FinalizationQueue = HashMap<BlockHeight, Vec<DeployHash>>;

/// A queue of requests we can't respond to yet, because we aren't up to date on finalized blocks.
/// The key is the height of the next block we will expect to be finalized at the point when we can
/// fulfill the corresponding requests.
type RequestQueue = HashMap<BlockHeight, Vec<ProtoBlockRequest>>;

/// Current operational state of a block proposer.
#[derive(DataSize, Debug)]
#[allow(clippy::large_enum_variant)]
enum BlockProposerState {
    /// Block proposer is initializing, waiting for a state snapshot.
    Initializing {
        /// Events cached pending transition to `Ready` state when they can be handled.
        pending: Vec<Event>,
        /// The key under which this component's state is cached in storage.
        state_key: Vec<u8>,
        /// The deploy config from the current chainspec.
        deploy_config: DeployConfig,
        /// The configuration, containing local settings for deploy selection
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
        // load the state from storage or use a fresh instance if loading fails.
        let state_key = deploy_sets::create_storage_key(chainspec);
        let effects = effect_builder
            .get_finalized_deploys(chainspec.deploy_config.max_ttl)
            .event(move |finalized_deploys| Event::Loaded {
                finalized_deploys,
                next_finalized_block,
            });

        let block_proposer = BlockProposer {
            state: BlockProposerState::Initializing {
                pending: Vec::new(),
                state_key,
                deploy_config: chainspec.deploy_config,
                local_config,
            },
            metrics: BlockProposerMetrics::new(registry)?,
        };

        Ok((block_proposer, effects))
    }
}

impl<REv> Component<REv> for BlockProposer
where
    REv: From<Event> + From<StorageRequest> + From<StateStoreRequest> + Send + 'static,
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
                    state_key,
                    deploy_config,
                    local_config,
                },
                Event::Loaded {
                    finalized_deploys,
                    next_finalized_block,
                },
            ) => {
                let mut new_ready_state = BlockProposerReady {
                    sets: BlockProposerDeploySets::from_finalized(
                        finalized_deploys,
                        next_finalized_block,
                    ),
                    unhandled_finalized: Default::default(),
                    deploy_config: *deploy_config,
                    state_key: state_key.clone(),
                    request_queue: Default::default(),
                    local_config: local_config.clone(),
                };

                // Replay postponed events onto new state.
                for ev in pending.drain(..) {
                    effects.extend(new_ready_state.handle_event(effect_builder, ev));
                }

                self.state = BlockProposerState::Ready(new_ready_state);

                // Start pruning deploys after delay.
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
                self.metrics
                    .pending_deploys
                    .set(ready_state.sets.pending.len() as i64);
            }
        };

        effects
    }
}

/// State of operational block proposer.
#[derive(DataSize, Debug)]
struct BlockProposerReady {
    /// Set of deploys currently stored in the block proposer.
    sets: BlockProposerDeploySets,
    /// `unhandled_finalized` is a set of hashes for deploys that the `BlockProposer` has not yet
    /// seen but were reported as reported to `finalized_deploys()`. They are used to
    /// filter deploys for proposal, similar to `self.sets.finalized_deploys`.
    unhandled_finalized: HashSet<DeployHash>,
    /// We don't need the whole Chainspec here, just the deploy config.
    deploy_config: DeployConfig,
    /// Key for storing the block proposer state.
    state_key: Vec<u8>,
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
        REv: Send + From<StateStoreRequest>,
    {
        match event {
            Event::Request(BlockProposerRequest::RequestProtoBlock(request)) => {
                if request.next_finalized > self.sets.next_finalized {
                    debug!(
                        request_next_finalized = %request.next_finalized,
                        self_next_finalized = %self.sets.next_finalized,
                        "received request before finalization announcement"
                    );
                    self.request_queue
                        .entry(request.next_finalized)
                        .or_default()
                        .push(request);
                    Effects::new()
                } else {
                    info!(%request.next_finalized, "proposing a proto block");
                    request
                        .responder
                        .respond(self.propose_proto_block(
                            self.deploy_config,
                            request.current_instant,
                            request.past_deploys,
                            request.random_bit,
                        ))
                        .ignore()
                }
            }
            Event::BufferDeploy { hash, deploy_type } => {
                self.add_deploy_or_transfer(Timestamp::now(), hash, *deploy_type);
                Effects::new()
            }
            Event::Prune => {
                let pruned = self.prune(Timestamp::now());
                debug!(%pruned, "pruned deploys from buffer");

                // Re-trigger timer after `PRUNE_INTERVAL`.
                effect_builder
                    .set_timeout(PRUNE_INTERVAL)
                    .event(|_| Event::Prune)
            }
            Event::Loaded { .. } => {
                // This should never happen, but we can just ignore the event and carry on.
                error!("got loaded event for block proposer state during ready state");
                Effects::new()
            }
            Event::FinalizedProtoBlock { block, mut height } => {
                let (_, mut deploys, transfers, _) = block.destructure();
                deploys.extend(transfers);

                if height > self.sets.next_finalized {
                    debug!(
                        %height,
                        next_finalized = %self.sets.next_finalized,
                        "received finalized blocks out of order; queueing"
                    );
                    // safe to subtract 1 - height will never be 0 in this branch, because
                    // next_finalized is at least 0, and height has to be greater
                    self.sets.finalization_queue.insert(height - 1, deploys);
                    Effects::new()
                } else {
                    debug!(%height, "handling finalized block");
                    let mut effects = self.handle_finalized_block(effect_builder, height, deploys);
                    while let Some(deploys) = self.sets.finalization_queue.remove(&height) {
                        info!(%height, "removed finalization queue entry");
                        height += 1;
                        effects.extend(self.handle_finalized_block(
                            effect_builder,
                            height,
                            deploys,
                        ));
                    }
                    effects
                }
            }
        }
    }

    /// Adds a deploy to the block proposer.
    ///
    /// Returns `false` if the deploy has been rejected.
    fn add_deploy_or_transfer(
        &mut self,
        current_instant: Timestamp,
        hash: DeployHash,
        deploy_or_transfer: DeployType,
    ) {
        if deploy_or_transfer.header().expired(current_instant) {
            trace!(%hash, "expired deploy rejected from the buffer");
            return;
        }
        if self.unhandled_finalized.remove(&hash) {
            info!(%hash,
                "deploy was previously marked as finalized, storing header"
            );
            self.sets
                .finalized_deploys
                .insert(hash, deploy_or_transfer.take_header());
            return;
        }
        // only add the deploy if it isn't contained in a finalized block
        if self.sets.finalized_deploys.contains_key(&hash) {
            info!(%hash, "deploy rejected from the buffer");
        } else {
            self.sets.pending.insert(hash, deploy_or_transfer);
            info!(%hash, "added deploy to the buffer");
        }
    }

    /// Notifies the block proposer that a block has been finalized.
    fn finalized_deploys<I>(&mut self, deploys: I)
    where
        I: IntoIterator<Item = DeployHash>,
    {
        for deploy_hash in deploys.into_iter() {
            match self.sets.pending.remove(&deploy_hash) {
                Some(deploy_type) => {
                    self.sets
                        .finalized_deploys
                        .insert(deploy_hash, deploy_type.take_header());
                }
                // If we haven't seen this deploy before, we still need to take note of it.
                _ => {
                    self.unhandled_finalized.insert(deploy_hash);
                }
            }
        }
    }

    /// Handles finalization of a block.
    fn handle_finalized_block<I, REv>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        height: BlockHeight,
        deploys: I,
    ) -> Effects<Event>
    where
        I: IntoIterator<Item = DeployHash>,
    {
        self.finalized_deploys(deploys);
        self.sets.next_finalized = self.sets.next_finalized.max(height + 1);

        if let Some(requests) = self.request_queue.remove(&self.sets.next_finalized) {
            info!(height = %(height + 1), "handling queued requests");
            requests
                .into_iter()
                .flat_map(|request| {
                    request
                        .responder
                        .respond(self.propose_proto_block(
                            self.deploy_config,
                            request.current_instant,
                            request.past_deploys,
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
    fn propose_proto_block(
        &mut self,
        deploy_config: DeployConfig,
        block_timestamp: Timestamp,
        past_deploys: HashSet<DeployHash>,
        random_bit: bool,
    ) -> ProtoBlock {
        let mut appendable_block = AppendableBlock::new(deploy_config, block_timestamp);

        // We prioritize transfers over deploys, so we try to include them first.
        for (hash, deploy_type) in &self.sets.pending {
            if !deploy_type.is_transfer()
                || !self.deps_resolved(&deploy_type.header(), &past_deploys)
                || past_deploys.contains(hash)
                || self.contains_finalized(hash)
            {
                continue;
            }

            if let Err(err) = appendable_block.add(*hash, deploy_type) {
                match err {
                    // We added the maximum number of transfers.
                    AddError::TransferCount | AddError::GasLimit | AddError::BlockSize => break,
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
        for (hash, deploy_type) in &self.sets.pending {
            if deploy_type.is_transfer()
                || !self.deps_resolved(&deploy_type.header(), &past_deploys)
                || past_deploys.contains(hash)
                || self.contains_finalized(hash)
            {
                continue;
            }

            if let Err(err) = appendable_block.add(*hash, deploy_type) {
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

        appendable_block.into_proto_block(random_bit)
    }

    /// Prunes expired deploy information from the BlockProposer, returns the total deploys pruned.
    fn prune(&mut self, current_instant: Timestamp) -> usize {
        self.sets.prune(current_instant)
    }

    fn contains_finalized(&self, dep: &DeployHash) -> bool {
        self.sets.finalized_deploys.contains_key(dep) || self.unhandled_finalized.contains(dep)
    }
}
