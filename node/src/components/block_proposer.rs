//! Block proposer.
//!
//! The block proposer stores deploy hashes in memory, tracking their suitability for inclusion into
//! a new block. Upon request, it returns a list of candidates that can be included.

mod deploy_sets;
mod event;
mod metrics;

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    time::Duration,
};

use datasize::DataSize;
use prometheus::{self, Registry};
use tracing::{debug, info, trace, warn};

use crate::{
    components::{chainspec_loader::DeployConfig, Component},
    effect::{
        requests::{
            BlockProposerRequest, ListForInclusionRequest, StateStoreRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    types::{DeployHash, DeployHeader, Timestamp},
    NodeRng,
};
pub(crate) use deploy_sets::BlockProposerDeploySets;
pub(crate) use event::Event;
use metrics::BlockProposerMetrics;
use semver::Version;

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

/// The type of values expressing the block height in the chain.
type BlockHeight = u64;

/// A collection of deploy hashes with their corresponding deploy headers.
type DeployCollection = HashMap<DeployHash, DeployHeader>;

/// A queue of contents of blocks that we know have been finalized, but we are still missing
/// notifications about finalization of some of their ancestors. It maps block height to the
/// deploys contained in the corresponding block.
type FinalizationQueue = HashMap<BlockHeight, Vec<DeployHash>>;

/// A queue of requests we can't respond to yet, because we aren't up to date on finalized blocks.
/// The key is the height of the next block we will expect to be finalized at the point when we can
/// fulfill the corresponding requests.
type RequestQueue = HashMap<BlockHeight, Vec<ListForInclusionRequest>>;

/// Current operational state of a block proposer.
#[derive(DataSize, Debug)]
#[allow(clippy::large_enum_variant)]
enum BlockProposerState {
    /// Block proposer is initializing, waiting for a state snapshot.
    Initializing { pending: Vec<Event> },
    /// Normal operation.
    Ready(BlockProposerReady),
}

impl BlockProposer {
    /// Creates a new block proposer instance.
    pub(crate) fn new<REv>(
        registry: Registry,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<(Self, Effects<Event>), prometheus::Error>
    where
        REv: From<Event> + From<StorageRequest> + From<StateStoreRequest> + Send + 'static,
    {
        // Note: Version is currently not honored by the storage component, so we just hardcode
        // 1.0.0.
        let effects = async move {
            let chainspec = effect_builder
                .get_chainspec(Version::new(1, 0, 0))
                .await
                // Note: Currently the storage component will always return a chainspec, however the
                // interface has not kept up with this yet.
                .expect("chainspec should be infallible");

            // With the chainspec, we can now load the state from storage or use a fresh instance if
            // loading fails.
            let key = deploy_sets::create_storage_key(&chainspec);
            let sets = effect_builder
                .load_state(key.into())
                .await
                .unwrap_or_default();

            (chainspec, sets)
        }
        .event(|(chainspec, sets)| Event::Loaded { chainspec, sets });

        let block_proposer = BlockProposer {
            state: BlockProposerState::Initializing {
                pending: Vec::new(),
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
                BlockProposerState::Initializing { ref mut pending },
                Event::Loaded { chainspec, sets },
            ) => {
                let mut new_ready_state = BlockProposerReady {
                    sets: sets.unwrap_or_default(),
                    deploy_config: chainspec.genesis.deploy_config,
                    state_key: deploy_sets::create_storage_key(&chainspec),
                    request_queue: Default::default(),
                };

                // Replay postponed events onto new state.
                for ev in pending.drain(0..pending.len()) {
                    effects.extend(new_ready_state.handle_event(effect_builder, ev));
                }

                self.state = BlockProposerState::Ready(new_ready_state);

                // Start pruning deploys after delay.
                effects.extend(
                    effect_builder
                        .set_timeout(PRUNE_INTERVAL)
                        .event(|_| Event::BufferPrune),
                );
            }
            (BlockProposerState::Initializing { ref mut pending }, event) => {
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
    // We don't need the whole Chainspec here, just the deploy config.
    deploy_config: DeployConfig,
    /// Key for storing the block proposer state.
    state_key: Vec<u8>,
    /// The queue of requests awaiting being handled.
    request_queue: RequestQueue,
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
            Event::Request(BlockProposerRequest::ListForInclusion(request)) => {
                if request.next_finalized > self.sets.next_finalized {
                    self.request_queue
                        .entry(request.next_finalized)
                        .or_default()
                        .push(request);
                    Effects::new()
                } else {
                    request
                        .responder
                        .respond(self.propose_deploys(
                            self.deploy_config,
                            request.current_instant,
                            request.past_deploys,
                        ))
                        .ignore()
                }
            }
            Event::Buffer { hash, header } => {
                self.add_deploy(Timestamp::now(), hash, *header);
                Effects::new()
            }
            Event::BufferPrune => {
                let pruned = self.prune(Timestamp::now());
                debug!("Pruned {} deploys from buffer", pruned);

                // After pruning, we store a state snapshot.
                let mut effects = effect_builder
                    .save_state(self.state_key.clone().into(), self.sets.clone())
                    .ignore();

                // Re-trigger timer after `PRUNE_INTERVAL`.
                effects.extend(
                    effect_builder
                        .set_timeout(PRUNE_INTERVAL)
                        .event(|_| Event::BufferPrune),
                );

                effects
            }
            Event::Loaded { sets, .. } => {
                // This should never happen, but we can just ignore the event and carry on.
                warn!(
                    ?sets,
                    "got loaded event for block proposer state during ready state"
                );
                Effects::new()
            }
            Event::FinalizedProtoBlock { block, mut height } => {
                let (_, deploys, _) = block.destructure();
                if height > self.sets.next_finalized {
                    // safe to subtract 1 - height will never be 0 in this branch, because
                    // next_finalized is at least 0, and height has to be greater
                    self.sets.finalization_queue.insert(height - 1, deploys);
                    Effects::new()
                } else {
                    let mut effects = self.handle_finalized_block(effect_builder, height, deploys);
                    while let Some(deploys) = self.sets.finalization_queue.remove(&height) {
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
    fn add_deploy(&mut self, current_instant: Timestamp, hash: DeployHash, header: DeployHeader) {
        if header.expired(current_instant) {
            trace!("expired deploy {} rejected from the buffer", hash);
            return;
        }
        // only add the deploy if it isn't contained in a finalized block
        if !self.sets.finalized_deploys.contains_key(&hash) {
            self.sets.pending.insert(hash, header);
            info!("added deploy {} to the buffer", hash);
        } else {
            info!("deploy {} rejected from the buffer", hash);
        }
    }

    /// Notifies the block proposer that a block has been finalized.
    fn finalized_deploys<I>(&mut self, deploys: I)
    where
        I: IntoIterator<Item = DeployHash>,
    {
        // TODO: This will ignore deploys that weren't in `pending`. They might be added
        // later, and then would be proposed as duplicates.
        let deploys: HashMap<_, _> = deploys
            .into_iter()
            .filter_map(|deploy_hash| {
                self.sets
                    .pending
                    .get(&deploy_hash)
                    .map(|deploy_header| (deploy_hash, deploy_header.clone()))
            })
            .collect();
        self.sets
            .pending
            .retain(|deploy_hash, _| !deploys.contains_key(deploy_hash));
        self.sets.finalized_deploys.extend(deploys);
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
        self.sets.next_finalized = height + 1;

        if let Some(requests) = self.request_queue.remove(&self.sets.next_finalized) {
            requests
                .into_iter()
                .flat_map(|request| {
                    request
                        .responder
                        .respond(self.propose_deploys(
                            self.deploy_config,
                            request.current_instant,
                            request.past_deploys,
                        ))
                        .ignore()
                })
                .collect()
        } else {
            Effects::new()
        }
    }

    /// Checks if a deploy is valid (for inclusion into the next block).
    fn is_deploy_valid(
        &self,
        deploy: &DeployHeader,
        block_timestamp: Timestamp,
        deploy_config: &DeployConfig,
        past_deploys: &HashSet<DeployHash>,
    ) -> bool {
        let all_deps_resolved = || {
            deploy.dependencies().iter().all(|dep| {
                past_deploys.contains(dep) || self.sets.finalized_deploys.contains_key(dep)
            })
        };
        let ttl_valid = deploy.ttl() <= deploy_config.max_ttl;
        let timestamp_valid = deploy.timestamp() <= block_timestamp;
        let deploy_valid = deploy.timestamp() + deploy.ttl() >= block_timestamp;
        let num_deps_valid = deploy.dependencies().len() <= deploy_config.max_dependencies as usize;
        ttl_valid && timestamp_valid && deploy_valid && num_deps_valid && all_deps_resolved()
    }

    /// Returns a list of candidates for inclusion into a block.
    // TODO:
    /// rename to proposed deploys
    // TODO:
    /// maybe use cuckoofilter
    fn propose_deploys(
        &mut self,
        deploy_config: DeployConfig,
        block_timestamp: Timestamp,
        past_deploys: HashSet<DeployHash>,
    ) -> HashSet<DeployHash> {
        // deploys_to_return = all deploys in pending that aren't in finalized blocks or
        // proposed blocks from the set `past_blocks`
        self.sets
            .pending
            .iter()
            .filter(|&(hash, deploy)| {
                self.is_deploy_valid(deploy, block_timestamp, &deploy_config, &past_deploys)
                    && !past_deploys.contains(hash)
                    && !self.sets.finalized_deploys.contains_key(hash)
            })
            .map(|(hash, _deploy)| *hash)
            .take(deploy_config.block_max_deploy_count as usize)
            .collect::<HashSet<_>>()
        // TODO: check gas and block size limits
    }

    /// Prunes expired deploy information from the BlockProposer, returns the total deploys pruned.
    fn prune(&mut self, current_instant: Timestamp) -> usize {
        self.sets.prune(current_instant)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
    use casper_types::bytesrepr::Bytes;

    use super::*;
    use crate::{
        crypto::asymmetric_key::SecretKey,
        testing::TestRng,
        types::{Deploy, DeployHash, DeployHeader, TimeDiff},
    };

    fn generate_deploy(
        rng: &mut TestRng,
        timestamp: Timestamp,
        ttl: TimeDiff,
        dependencies: Vec<DeployHash>,
    ) -> (DeployHash, DeployHeader) {
        let secret_key = SecretKey::random(rng);
        let gas_price = 10;
        let chain_name = "chain".to_string();
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: Bytes::new(),
        };
        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: Bytes::new(),
        };

        let deploy = Deploy::new(
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            payment,
            session,
            &secret_key,
            rng,
        );

        (*deploy.id(), deploy.take_header())
    }

    fn create_test_proposer() -> BlockProposerReady {
        BlockProposerReady {
            sets: Default::default(),
            deploy_config: Default::default(),
            state_key: b"block-proposer-test".to_vec(),
            request_queue: Default::default(),
        }
    }

    impl From<StorageRequest> for Event {
        fn from(_: StorageRequest) -> Self {
            // we never send a storage request in our unit tests, but if this does become
            // meaningful....
            unreachable!("no storage requests in block proposer unit tests")
        }
    }

    impl From<StateStoreRequest> for Event {
        fn from(_: StateStoreRequest) -> Self {
            unreachable!("no state store requests in block proposer unit tests")
        }
    }

    #[test]
    fn add_and_take_deploys() {
        let creation_time = Timestamp::from(100);
        let ttl = TimeDiff::from(100);
        let block_time1 = Timestamp::from(80);
        let block_time2 = Timestamp::from(120);
        let block_time3 = Timestamp::from(220);

        let no_deploys = HashSet::new();
        let mut proposer = create_test_proposer();
        let mut rng = crate::new_rng();
        let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        let (hash3, deploy3) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        let (hash4, deploy4) = generate_deploy(&mut rng, creation_time, ttl, vec![]);

        assert!(proposer
            .propose_deploys(DeployConfig::default(), block_time2, no_deploys.clone())
            .is_empty());

        // add two deploys
        proposer.add_deploy(block_time2, hash1, deploy1);
        proposer.add_deploy(block_time2, hash2, deploy2);

        // if we try to create a block with a timestamp that is too early, we shouldn't get any
        // deploys
        assert!(proposer
            .propose_deploys(DeployConfig::default(), block_time1, no_deploys.clone())
            .is_empty());

        // if we try to create a block with a timestamp that is too late, we shouldn't get any
        // deploys, either
        assert!(proposer
            .propose_deploys(DeployConfig::default(), block_time3, no_deploys.clone())
            .is_empty());

        // take the deploys out
        let deploys =
            proposer.propose_deploys(DeployConfig::default(), block_time2, no_deploys.clone());

        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains(&hash1));
        assert!(deploys.contains(&hash2));

        assert_eq!(
            proposer
                .propose_deploys(DeployConfig::default(), block_time2, no_deploys.clone())
                .len(),
            2
        );

        // but they shouldn't be returned if we include it in the past deploys
        assert!(proposer
            .propose_deploys(DeployConfig::default(), block_time2, deploys.clone())
            .is_empty());

        // finalize the block
        proposer.finalized_deploys(deploys);

        // add more deploys
        proposer.add_deploy(block_time2, hash3, deploy3);
        proposer.add_deploy(block_time2, hash4, deploy4);

        let deploys = proposer.propose_deploys(DeployConfig::default(), block_time2, no_deploys);

        // since block 1 is now finalized, neither deploy1 nor deploy2 should be among the returned
        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains(&hash3));
        assert!(deploys.contains(&hash4));
    }

    #[test]
    fn test_prune() {
        let expired_time = Timestamp::from(201);
        let creation_time = Timestamp::from(100);
        let test_time = Timestamp::from(120);
        let ttl = TimeDiff::from(100);

        let mut rng = crate::new_rng();
        let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        let (hash3, deploy3) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        let (hash4, deploy4) = generate_deploy(
            &mut rng,
            creation_time + Duration::from_secs(20).into(),
            ttl,
            vec![],
        );
        let mut proposer = create_test_proposer();

        // pending
        proposer.add_deploy(creation_time, hash1, deploy1);
        proposer.add_deploy(creation_time, hash2, deploy2);
        proposer.add_deploy(creation_time, hash3, deploy3);
        proposer.add_deploy(creation_time, hash4, deploy4);

        // pending => finalized
        proposer.finalized_deploys(vec![hash1]);

        assert_eq!(proposer.sets.pending.len(), 3);
        assert!(proposer.sets.finalized_deploys.contains_key(&hash1));

        // test for retained values
        let pruned = proposer.prune(test_time);
        assert_eq!(pruned, 0);

        assert_eq!(proposer.sets.pending.len(), 3);
        assert_eq!(proposer.sets.finalized_deploys.len(), 1);
        assert!(proposer.sets.finalized_deploys.contains_key(&hash1));

        // now move the clock to make some things expire
        let pruned = proposer.prune(expired_time);
        assert_eq!(pruned, 3);

        assert_eq!(proposer.sets.pending.len(), 1); // deploy4 is still valid
        assert_eq!(proposer.sets.finalized_deploys.len(), 0);
    }

    #[test]
    fn test_deploy_dependencies() {
        let creation_time = Timestamp::from(100);
        let ttl = TimeDiff::from(100);
        let block_time = Timestamp::from(120);

        let mut rng = crate::new_rng();
        let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        // let deploy2 depend on deploy1
        let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![hash1]);

        let no_deploys = HashSet::new();
        let mut proposer = create_test_proposer();

        // add deploy2
        proposer.add_deploy(creation_time, hash2, deploy2);

        // deploy2 has an unsatisfied dependency
        assert!(proposer
            .propose_deploys(DeployConfig::default(), block_time, no_deploys.clone())
            .is_empty());

        // add deploy1
        proposer.add_deploy(creation_time, hash1, deploy1);

        let deploys =
            proposer.propose_deploys(DeployConfig::default(), block_time, no_deploys.clone());
        // only deploy1 should be returned, as it has no dependencies
        assert_eq!(deploys.len(), 1);
        assert!(deploys.contains(&hash1));

        // the deploy will be included in block 1
        proposer.finalized_deploys(deploys);

        let deploys2 = proposer.propose_deploys(DeployConfig::default(), block_time, no_deploys);
        // `blocks` contains a block that contains deploy1 now, so we should get deploy2
        assert_eq!(deploys2.len(), 1);
        assert!(deploys2.contains(&hash2));
    }
}
