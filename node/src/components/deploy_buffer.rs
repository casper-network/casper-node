mod config;
mod event;
mod metrics;

use std::{
    collections::{btree_map, BTreeMap, BTreeSet, HashMap, HashSet},
    convert::TryInto,
    iter::FromIterator,
    mem,
};

use datasize::DataSize;
use futures::FutureExt;
use itertools::Itertools;
use prometheus::Registry;
use smallvec::smallvec;
use tracing::{debug, error, info, warn};

use casper_types::Timestamp;

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        Component, ComponentStatus, InitializedComponent,
    },
    effect::{
        announcements::DeployBufferAnnouncement,
        requests::{DeployBufferRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    reactor::main_reactor::MainEvent,
    storage::Storage,
    types::{
        appendable_block::{AddError, AppendableBlock},
        chainspec::DeployConfig,
        Approval, Block, BlockHeader, Deploy, DeployFootprint, DeployHash, DeployHashWithApprovals,
        DeployId, FinalizedBlock,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;

use metrics::Metrics;

type FootprintAndApprovals = (DeployFootprint, BTreeSet<Approval>);

#[derive(DataSize, Debug)]
pub(crate) struct DeployBuffer {
    status: ComponentStatus,
    cfg: Config,
    deploy_config: DeployConfig,
    // Keeps track of all deploys the buffer is currently aware of.
    //
    // `hold` and `dead` are used to filter it on demand as necessary.
    //
    // The timestamp is the time when the deploy expires.
    // Expired items are removed via a self-perpetuating expire event.
    buffer: HashMap<DeployHash, (Timestamp, Option<FootprintAndApprovals>)>,
    // when a maybe-block is in flight, we pause inclusion
    // of the deploys within it in other proposed blocks
    // if the maybe-block becomes an actual block the
    // deploy hashes will get put to self.dead
    // otherwise, the hold will be released and the deploys
    // will become eligible to propose again.
    hold: BTreeMap<Timestamp, HashSet<DeployHash>>,
    // deploy_hashes that should not be proposed, ever
    dead: HashSet<DeployHash>,
    // block_height and block_time of blocks added to the local chain needed to ensure we have seen
    // sufficient blocks to allow consensus to proceed safely without risking duplicating deploys.
    //
    // if we have gaps in block_height, we do NOT have full TTL awareness.
    // if we have no block_height gaps but our earliest block_time is less than now - ttl,
    // we do NOT have full TTL awareness.
    chain_index: BTreeMap<u64, Timestamp>,
    // deploy buffer metrics
    #[data_size(skip)]
    metrics: Metrics,
}

impl DeployBuffer {
    /// Create a deploy buffer for fun and profit.
    pub(crate) fn new(
        deploy_config: DeployConfig,
        cfg: Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(DeployBuffer {
            status: ComponentStatus::Uninitialized,
            cfg,
            deploy_config,
            buffer: HashMap::new(),
            hold: BTreeMap::new(),
            dead: HashSet::new(),
            chain_index: BTreeMap::new(),
            metrics: Metrics::new(registry)?,
        })
    }

    pub(crate) fn initialize_component(
        &self,
        effect_builder: EffectBuilder<MainEvent>,
        storage: &Storage,
    ) -> Option<Effects<MainEvent>> {
        if <DeployBuffer as InitializedComponent<MainEvent>>::is_uninitialized(self) {
            let blocks = match storage.read_blocks_for_replay_protection() {
                Ok(blocks) => blocks,
                Err(err) => {
                    return Some(
                        fatal!(
                            effect_builder,
                            "fatal block store error when attempting to read highest blocks: {}",
                            err
                        )
                        .ignore(),
                    )
                }
            };
            debug!(
                blocks = ?blocks.iter().map(|b| b.height()).collect_vec(),
                "DeployBuffer: initialization"
            );
            info!(
                "initialized {}",
                <DeployBuffer as InitializedComponent<MainEvent>>::name(self)
            );
            let event = Event::Initialize(blocks);
            return Some(smallvec![async {
                smallvec![MainEvent::DeployBuffer(event)]
            }
            .boxed()]);
        }
        if <DeployBuffer as InitializedComponent<MainEvent>>::is_fatal(self) {
            return Some(
                fatal!(
                    effect_builder,
                    "{} failed to initialize",
                    <DeployBuffer as InitializedComponent<MainEvent>>::name(self)
                )
                .ignore(),
            );
        }
        None
    }

    /// Returns `true` if we have enough information to participate in consensus in the era after
    /// the `switch block`.
    ///
    /// To validate and propose blocks we need to be able to avoid and detect deploy replays.
    /// Each block in the new era E will have a timestamp greater or equal to E's start time T,
    /// which is the switch block's timestamp. A deploy D in a block B cannot have timed out as of
    /// B's timestamp, so it cannot be older than T - ttl, where ttl is the maximum deploy
    /// time-to-live. So if D were also in an earlier block C, then C's timestamp would be at
    /// least T - ttl. Thus to prevent replays, we need all blocks with a timestamp between
    /// T - ttl and T.
    ///
    /// Note that we don't need to already have any blocks from E itself, since the consensus
    /// protocol will announce all proposed and finalized blocks in E anyway.
    pub(crate) fn have_full_ttl_of_deploys(&self, switch_block: &BlockHeader) -> bool {
        let from_height = switch_block.height();
        let earliest_needed = switch_block
            .timestamp()
            .saturating_sub(self.deploy_config.max_ttl);
        debug!(
            ?self.chain_index, %earliest_needed,
            "DeployBuffer: chain_index check from_height: {}", from_height
        );
        let mut current = match self.chain_index.get(&from_height) {
            None => {
                debug!("DeployBuffer: not in chain_index: {}", from_height);
                return false;
            }
            Some(timestamp) => (from_height, timestamp),
        };

        // genesis special case
        if from_height == 0
            && self.chain_index.len() == 1
            && self.chain_index.contains_key(&from_height)
        {
            return true;
        }

        // note the .rev() at the end (we go backwards)
        // I only mention it as it seems to keep tripping people up. Not YOU of course, but people.
        for (height, timestamp) in self.chain_index.range(..from_height).rev() {
            // if we've reached genesis via an unbroken sequence, we're good
            // if we've seen a full ttl period of blocks, we're good
            if *height == 0 || *timestamp < earliest_needed {
                return true;
            }
            // gap detection; any gaps in the chain index means we do not have full ttl awareness
            // for instance, for block_height 6 in set [0,1,2,4,5,6] block 3 is missing
            // therefore, we can't possibly have ttl
            let contiguous_next = height.saturating_add(1);
            if contiguous_next != current.0 {
                debug!("DeployBuffer: missing block at height: {}", contiguous_next);
                return false;
            }
            current = (*height, timestamp);
        }
        false
    }

    /// Manages cache ejection.
    fn expire<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<Event> + From<DeployBufferAnnouncement> + Send,
    {
        let now = Timestamp::now();
        let (buffer, freed): (HashMap<_, _>, _) = mem::take(&mut self.buffer)
            .into_iter()
            .partition(|(_, (expiry_time, _))| *expiry_time >= now);

        let freed_count = freed.len();
        if freed_count > 0 {
            info!("DeployBuffer: expiring {} deploys", freed_count);
        }

        // clear expired deploy from all holds, then clear any entries that have no items remaining
        self.hold.iter_mut().for_each(|(_, held_deploys)| {
            held_deploys.retain(|deploy_hash| !freed.contains_key(deploy_hash))
        });
        self.hold.retain(|_, remaining| !remaining.is_empty());

        self.dead
            .retain(|deploy_hash| !freed.contains_key(deploy_hash));
        self.buffer = buffer;

        let mut effects = effect_builder
            .announce_expired_deploys(freed.keys().cloned().collect())
            .ignore();
        effects.extend(
            effect_builder
                .set_timeout(self.cfg.expiry_check_interval().into())
                .event(move |_| Event::Expire),
        );
        self.update_all_metrics();
        effects
    }

    fn register_deploy_gossiped<REv>(
        &mut self,
        deploy_id: DeployId,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<Event> + From<StorageRequest> + Send,
    {
        debug!(%deploy_id, "DeployBuffer: registering gossiped deploy");
        effect_builder
            .get_stored_deploy(deploy_id)
            .event(move |result| Event::StoredDeploy(deploy_id, Box::new(result)))
    }

    /// Update buffer considering new stored deploy.
    fn register_deploy(&mut self, deploy: Deploy) {
        let deploy_hash = deploy.hash();
        if deploy.is_valid().is_err() {
            error!(%deploy_hash, "DeployBuffer: invalid deploy must not be buffered");
            return;
        }
        if self.dead.contains(deploy_hash) {
            info!(%deploy_hash, "DeployBuffer: attempt to register already dead deploy");
            return;
        }
        if self.hold.values().any(|dhs| dhs.contains(deploy_hash)) {
            info!(%deploy_hash, "DeployBuffer: attempt to register already held deploy");
            return;
        }
        let footprint = match deploy.footprint() {
            Ok(footprint) => footprint,
            Err(err) => {
                error!(%deploy_hash, %err, "DeployBuffer: deploy footprint exceeds tolerances");
                return;
            }
        };
        let expiry_time = deploy.header().expires();
        let approvals = deploy.approvals().clone();
        match self
            .buffer
            .insert(*deploy_hash, (expiry_time, Some((footprint, approvals))))
        {
            Some(prev) => {
                warn!(%deploy_hash, ?prev, "DeployBuffer: deploy upserted");
            }
            None => {
                debug!(%deploy_hash, "DeployBuffer: new deploy buffered");
                self.metrics.total_deploys.inc();
            }
        }
    }

    /// Update holds considering new proposed block.
    fn register_block_proposed(&mut self, proposed_block: ProposedBlock<ClContext>) {
        let timestamp = &proposed_block.context().timestamp();
        if let Some(hold_set) = self.hold.get_mut(timestamp) {
            debug!(%timestamp, "DeployBuffer: existing hold timestamp extended");
            hold_set.extend(proposed_block.value().deploy_and_transfer_hashes());
        } else {
            debug!(%timestamp, "DeployBuffer: new hold timestamp inserted");
            self.hold.insert(
                *timestamp,
                HashSet::from_iter(proposed_block.value().deploy_and_transfer_hashes().copied()),
            );
        }
        self.metrics.held_deploys.set(
            self.hold
                .values()
                .map(|deploys| deploys.len())
                .sum::<usize>()
                .try_into()
                .unwrap_or(i64::MIN),
        );
    }

    fn register_deploys<'a>(
        &mut self,
        block_height: u64,
        timestamp: Timestamp,
        deploy_hashes: impl Iterator<Item = &'a DeployHash>,
    ) {
        self.chain_index.insert(block_height, timestamp);
        let expiry_timestamp = timestamp.saturating_add(self.deploy_config.max_ttl);

        for deploy_hash in deploy_hashes {
            if !self.buffer.contains_key(deploy_hash) {
                self.buffer.insert(*deploy_hash, (expiry_timestamp, None));
            }
            self.dead.insert(*deploy_hash);
        }
        // deploys held for proposed blocks which did not get finalized in time are eligible again
        let (hold, _) = mem::take(&mut self.hold)
            .into_iter()
            .partition(|(ts, _)| *ts > timestamp);
        self.hold = hold;
        self.update_all_metrics();
    }

    /// Update buffer and holds considering new added block.
    fn register_block(&mut self, block: &Block) {
        let block_height = block.header().height();
        let timestamp = block.timestamp();
        debug!(%timestamp, "DeployBuffer: register_block({}) timestamp finalized", block_height);
        self.register_deploys(block_height, timestamp, block.deploy_and_transfer_hashes());
    }

    /// Update buffer and holds considering new finalized block.
    fn register_block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        let block_height = finalized_block.height();
        let timestamp = finalized_block.timestamp();
        debug!(%timestamp, "DeployBuffer: register_block_finalized({}) timestamp finalized", block_height);
        self.register_deploys(
            block_height,
            timestamp,
            finalized_block.deploy_and_transfer_hashes(),
        );
    }

    /// Returns eligible deploys that are buffered and not held or dead.
    fn proposable(&self) -> Vec<(DeployHashWithApprovals, DeployFootprint)> {
        debug!("DeployBuffer: getting proposable deploys");
        self.buffer
            .iter()
            .filter(|(dh, _)| !self.hold.values().any(|hs| hs.contains(dh)))
            .filter(|(dh, _)| !self.dead.contains(dh))
            .filter_map(|(dh, (_, maybe_data))| {
                maybe_data.as_ref().map(|(footprint, approvals)| {
                    (
                        DeployHashWithApprovals::new(*dh, approvals.clone()),
                        footprint.clone(),
                    )
                })
            })
            .collect()
    }

    /// Returns a right-sized payload of deploys that can be proposed.
    fn appendable_block(&mut self, timestamp: Timestamp) -> AppendableBlock {
        let mut ret = AppendableBlock::new(self.deploy_config, timestamp);
        let mut holds = HashSet::new();
        let mut have_hit_transfer_limit = false;
        let mut have_hit_deploy_limit = false;
        for (with_approvals, footprint) in self.proposable() {
            if footprint.is_transfer && have_hit_transfer_limit {
                continue;
            }
            if !footprint.is_transfer && have_hit_deploy_limit {
                continue;
            }
            let deploy_hash = *with_approvals.deploy_hash();
            let has_multiple_approvals = with_approvals.approvals().len() > 1;
            match ret.add(with_approvals, &footprint) {
                Ok(_) => {
                    debug!(%deploy_hash, "DeployBuffer: proposing deploy");
                    holds.insert(deploy_hash);
                }
                Err(error) => {
                    match error {
                        AddError::Duplicate => {
                            // it should be physically impossible for a duplicate deploy to
                            // be in the deploy buffer, thus this should be unreachable
                            error!(
                                ?deploy_hash,
                                "DeployBuffer: duplicated deploy in deploy buffer"
                            );
                            self.dead.insert(deploy_hash);
                        }
                        AddError::InvalidDeploy => {
                            // it should not be possible for an invalid deploy to get buffered
                            // in the first place, thus this should be unreachable
                            error!(
                                ?deploy_hash,
                                "DeployBuffer: invalid deploy in deploy buffer"
                            );
                            self.dead.insert(deploy_hash);
                        }
                        AddError::TransferCount => {
                            if have_hit_deploy_limit {
                                info!(
                                    ?deploy_hash,
                                    "DeployBuffer: block filled with transfers and deploys"
                                );
                                break;
                            }
                            have_hit_transfer_limit = true;
                        }
                        AddError::DeployCount => {
                            if have_hit_transfer_limit {
                                info!(
                                    ?deploy_hash,
                                    "DeployBuffer: block filled with deploys and transfers"
                                );
                                break;
                            }
                            have_hit_deploy_limit = true;
                        }
                        AddError::ApprovalCount if has_multiple_approvals => {
                            // keep iterating, we can maybe fit in a deploy with fewer approvals
                        }
                        AddError::ApprovalCount | AddError::GasLimit | AddError::BlockSize => {
                            info!(
                                ?deploy_hash,
                                %error,
                                "DeployBuffer: a block limit has been reached"
                            );
                            // a block limit has been reached
                            break;
                        }
                    }
                }
            }
        }

        // put a hold on all proposed deploys / transfers and update metrics
        match self.hold.entry(timestamp) {
            btree_map::Entry::Vacant(entry) => {
                entry.insert(holds);
            }
            btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().extend(holds);
            }
        }
        self.update_all_metrics();

        info!(
            "produced {}, buffer has {} held, {} dead, {} total",
            ret,
            self.hold
                .values()
                .map(|deploys| deploys.len())
                .sum::<usize>(),
            self.dead.len(),
            self.buffer.len()
        );

        ret
    }

    /// Updates all deploy count metrics based on the size of the internal structs.
    fn update_all_metrics(&mut self) {
        // if number of elements is too high to fit, we overflow the metric
        // intentionally in order to get some indication that something is wrong.
        self.metrics.held_deploys.set(
            self.hold
                .values()
                .map(|deploys| deploys.len())
                .sum::<usize>()
                .try_into()
                .unwrap_or(i64::MIN),
        );
        self.metrics
            .dead_deploys
            .set(self.dead.len().try_into().unwrap_or(i64::MIN));
        self.metrics
            .total_deploys
            .set(self.buffer.len().try_into().unwrap_or(i64::MIN));
    }
}

impl<REv> InitializedComponent<REv> for DeployBuffer
where
    REv: From<Event> + From<DeployBufferAnnouncement> + From<StorageRequest> + Send + 'static,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }

    fn name(&self) -> &str {
        "deploy_buffer"
    }
}

impl<REv> Component<REv> for DeployBuffer
where
    REv: From<Event> + From<DeployBufferAnnouncement> + From<StorageRequest> + Send + 'static,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.status {
            ComponentStatus::Fatal(msg) => {
                warn!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentStatus::Uninitialized => {
                if let Event::Initialize(blocks) = event {
                    for block in blocks {
                        self.register_block(&block);
                    }
                    self.status = ComponentStatus::Initialized;
                    // start self-expiry management on initialization
                    effect_builder
                        .set_timeout(self.cfg.expiry_check_interval().into())
                        .event(move |_| Event::Expire)
                } else {
                    warn!(
                        ?event,
                        "should not handle this event when component is uninitialized"
                    );
                    Effects::new()
                }
            }
            ComponentStatus::Initialized => match event {
                Event::Initialize(_) => {
                    warn!("deploy buffer already initialized");
                    Effects::new()
                }
                Event::Request(DeployBufferRequest::GetAppendableBlock {
                    timestamp,
                    responder,
                }) => responder.respond(self.appendable_block(timestamp)).ignore(),
                Event::BlockFinalized(finalized_block) => {
                    self.register_block_finalized(&*finalized_block);
                    Effects::new()
                }
                Event::Block(block) => {
                    self.register_block(&block);
                    Effects::new()
                }
                Event::BlockProposed(proposed) => {
                    self.register_block_proposed(*proposed);
                    Effects::new()
                }
                Event::ReceiveDeployGossiped(deploy_id) => {
                    self.register_deploy_gossiped(deploy_id, effect_builder)
                }
                Event::StoredDeploy(deploy_id, maybe_deploy) => {
                    match *maybe_deploy {
                        Some(deploy) => {
                            self.register_deploy(deploy);
                        }
                        None => {
                            warn!("cannot register un-stored deploy({})", deploy_id);
                        }
                    }
                    Effects::new()
                }
                Event::Expire => self.expire(effect_builder),
            },
        }
    }
}
