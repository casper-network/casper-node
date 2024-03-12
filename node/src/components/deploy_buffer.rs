mod config;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::{
    collections::{btree_map, BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
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

use casper_hashing::Digest;
use casper_types::Timestamp;

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        Component, ComponentState, InitializedComponent,
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
        Approval, Block, Deploy, DeployFootprint, DeployHash, DeployHashWithApprovals, DeployId,
        FinalizedBlock,
    },
    utils::DisplayIter,
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;

use metrics::Metrics;

const COMPONENT_NAME: &str = "deploy_buffer";

type FootprintAndApprovals = (DeployFootprint, BTreeSet<Approval>);

#[derive(DataSize, Debug)]
pub(crate) struct DeployBuffer {
    state: ComponentState,
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
            state: ComponentState::Uninitialized,
            cfg,
            deploy_config,
            buffer: HashMap::new(),
            hold: BTreeMap::new(),
            dead: HashSet::new(),
            metrics: Metrics::new(registry)?,
        })
    }

    pub(crate) fn initialize_component(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        storage: &Storage,
    ) -> Option<Effects<MainEvent>> {
        if <Self as InitializedComponent<MainEvent>>::is_uninitialized(self) {
            info!(
                "pending initialization of {}",
                <Self as Component<MainEvent>>::name(self)
            );
            <Self as InitializedComponent<MainEvent>>::set_state(
                self,
                ComponentState::Initializing,
            );
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
            info!("initialized {}", <Self as Component<MainEvent>>::name(self));
            let event = Event::Initialize(blocks);
            return Some(smallvec![async {
                smallvec![MainEvent::DeployBuffer(event)]
            }
            .boxed()]);
        }
        if <Self as InitializedComponent<MainEvent>>::is_fatal(self) {
            return Some(
                fatal!(
                    effect_builder,
                    "{} failed to initialize",
                    <Self as Component<MainEvent>>::name(self)
                )
                .ignore(),
            );
        }
        None
    }

    /// Manages cache ejection.
    fn expire<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<Event> + From<DeployBufferAnnouncement> + Send,
    {
        let now = Timestamp::now();
        let (buffer, mut freed): (HashMap<_, _>, _) = mem::take(&mut self.buffer)
            .into_iter()
            .partition(|(_, (expiry_time, _))| *expiry_time >= now);

        if !freed.is_empty() {
            info!("DeployBuffer: purging {} deploy(s)", freed.len());
        }

        // clear expired deploy from all holds, then clear any entries that have no items remaining
        self.hold.iter_mut().for_each(|(_, held_deploys)| {
            held_deploys.retain(|deploy_hash| !freed.contains_key(deploy_hash))
        });
        self.hold.retain(|_, remaining| !remaining.is_empty());

        // retain all those in `dead` which are not in `freed`, at the same time reducing `freed` to
        // only those entries not also in `dead` - i.e. deploys which expired without being included
        // in a block
        self.dead
            .retain(|deploy_hash| freed.remove(deploy_hash).is_none());
        self.buffer = buffer;

        if !freed.is_empty() {
            info!(
                "DeployBuffer: expiring without executing {} deploy(s)",
                freed.len()
            );
            debug!(
                "DeployBuffer: expiring without executing {}",
                DisplayIter::new(freed.keys())
            );
        }

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
            .event(move |result| Event::StoredDeploy(deploy_id, result.map(Box::new)))
    }

    /// Update buffer considering new stored deploy.
    fn register_deploy(&mut self, deploy: Deploy) {
        let deploy_hash = deploy.hash();
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
        timestamp: Timestamp,
        deploy_hashes: impl Iterator<Item = &'a DeployHash>,
    ) {
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
        self.register_deploys(timestamp, block.deploy_and_transfer_hashes());
    }

    /// Update buffer and holds considering new finalized block.
    fn register_block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        let block_height = finalized_block.height();
        let timestamp = finalized_block.timestamp();
        debug!(%timestamp, "DeployBuffer: register_block_finalized({}) timestamp finalized", block_height);
        self.register_deploys(timestamp, finalized_block.deploy_and_transfer_hashes());
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

    fn buckets(&mut self) -> HashMap<Digest, Vec<(DeployHashWithApprovals, DeployFootprint)>> {
        let proposable = self.proposable();

        let mut buckets: HashMap<Digest, Vec<(DeployHashWithApprovals, DeployFootprint)>> =
            HashMap::new();

        for (with_approvals, footprint) in proposable {
            let body_hash = *footprint.header.body_hash();
            buckets
                .entry(body_hash)
                .and_modify(|vec| vec.push((with_approvals.clone(), footprint.clone())))
                .or_insert(vec![(with_approvals, footprint)]);
        }
        buckets
    }

    /// Returns a right-sized payload of deploys that can be proposed.
    fn appendable_block(
        &mut self,
        timestamp: Timestamp,
        request_expiry: Timestamp,
    ) -> AppendableBlock {
        let mut ret = AppendableBlock::new(self.deploy_config, timestamp);
        if Timestamp::now() >= request_expiry {
            return ret;
        }
        let mut holds = HashSet::new();
        let mut have_hit_transfer_limit = false;
        let mut have_hit_deploy_limit = false;

        let mut buckets = self.buckets();
        let mut body_hashes_queue: VecDeque<_> = buckets.keys().cloned().collect();

        #[cfg(test)]
        let mut iter_counter = 0;
        #[cfg(test)]
        let iter_limit = self.buffer.len() * 4;

        while let Some(body_hash) = body_hashes_queue.pop_front() {
            if Timestamp::now() > request_expiry {
                break;
            }
            #[cfg(test)]
            {
                iter_counter += 1;
                assert!(
                    iter_counter < iter_limit,
                    "the number of iterations shouldn't be too large"
                );
            }

            let Some((with_approvals, footprint)) =
                buckets.get_mut(&body_hash).and_then(Vec::<_>::pop)
            else {
                continue;
            };
            // bucket wasn't empty - push the hash back into the queue to be processed again on the
            // next pass
            body_hashes_queue.push_back(body_hash);
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
                        AddError::Expired => {
                            info!(
                                ?deploy_hash,
                                "DeployBuffer: expired deploy in deploy buffer"
                            );
                            self.dead.insert(deploy_hash);
                        }
                        AddError::InvalidDeploy => {
                            // It should not generally be possible for an invalid deploy to get
                            // buffered in the first place, thus this should be unreachable.  There
                            // is a small potential for a slightly future-dated deploy to be
                            // accepted (if within `timestamp_leeway`) and still be future-dated by
                            // the time we try and add it to a proposed block here.
                            warn!(
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
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
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
        match &self.state {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => {
                match event {
                    Event::Initialize(blocks) => {
                        for block in blocks {
                            self.register_block(&block);
                        }
                        <Self as InitializedComponent<MainEvent>>::set_state(
                            self,
                            ComponentState::Initialized,
                        );
                        // start self-expiry management on initialization
                        effect_builder
                            .set_timeout(self.cfg.expiry_check_interval().into())
                            .event(move |_| Event::Expire)
                    }
                    Event::Request(_)
                    | Event::ReceiveDeployGossiped(_)
                    | Event::StoredDeploy(_, _)
                    | Event::BlockProposed(_)
                    | Event::Block(_)
                    | Event::BlockFinalized(_)
                    | Event::Expire => {
                        warn!(
                            ?event,
                            name = <Self as Component<MainEvent>>::name(self),
                            "should not handle this event when component is pending initialization"
                        );
                        Effects::new()
                    }
                }
            }
            ComponentState::Initialized => match event {
                Event::Initialize(_) => {
                    error!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::Request(DeployBufferRequest::GetAppendableBlock {
                    timestamp,
                    request_expiry,
                    responder,
                }) => responder
                    .respond(self.appendable_block(timestamp, request_expiry))
                    .ignore(),
                Event::BlockFinalized(finalized_block) => {
                    self.register_block_finalized(&finalized_block);
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
                    match maybe_deploy {
                        Some(deploy) => {
                            self.register_deploy(*deploy);
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

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
