mod config;
mod event;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    iter::FromIterator,
    mem,
};

use datasize::DataSize;
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
    types::{
        appendable_block::{AddError, AppendableBlock},
        chainspec::DeployConfig,
        Approval, Deploy, DeployFootprint, DeployHash, DeployHashWithApprovals, DeployId,
        FinalizedBlock,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;

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
}

impl DeployBuffer {
    /// Create a deploy buffer for fun and profit.
    pub(crate) fn new(deploy_config: DeployConfig, cfg: Config) -> Self {
        DeployBuffer {
            status: ComponentStatus::Uninitialized,
            cfg,
            deploy_config,
            buffer: HashMap::new(),
            hold: BTreeMap::new(),
            dead: HashSet::new(),
            chain_index: BTreeMap::new(),
        }
    }

    /// True if we have full TTL worth of deploy awareness, else false.
    pub(crate) fn have_full_ttl_of_deploys(&self, from_height: u64) -> bool {
        let ttl = self.deploy_config.max_ttl;
        let mut curr = match self.chain_index.get(&from_height) {
            None => {
                return false;
            }
            Some(timestamp) => (from_height, timestamp),
        };

        // genesis special case
        if from_height == 0 {
            return true;
        }

        for (height, timestamp) in self.chain_index.range(..from_height).rev() {
            if height.saturating_add(1) != curr.0 {
                return false;
            }
            if timestamp.elapsed() > ttl || *height == 0 {
                return true;
            }
            curr = (*height, timestamp);
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

        debug!("DeployBuffer: expiring {} deploys", freed.len());
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
            }
        }
    }

    /// Update holds considering new proposed block.
    fn register_block_proposed(&mut self, proposed_block: ProposedBlock<ClContext>) {
        let timestamp = &proposed_block.context().timestamp();
        if let Some(hold_set) = self.hold.get_mut(timestamp) {
            debug!(%timestamp, "DeployBuffer: existing hold timestamp extended");
            hold_set.extend(proposed_block.value().deploy_and_transfer_hashes())
        } else {
            debug!(%timestamp, "DeployBuffer: new hold timestamp inserted");
            self.hold.insert(
                *timestamp,
                HashSet::from_iter(proposed_block.value().deploy_and_transfer_hashes().copied()),
            );
        }
    }

    /// Update buffer and holds considering new finalized block.
    fn register_block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        let timestamp = finalized_block.timestamp();
        self.chain_index.insert(finalized_block.height(), timestamp);
        let expiry_timestamp = timestamp.saturating_add(self.deploy_config.max_ttl);
        // all deploys in the finalized block must not be included in future proposals
        for hash in finalized_block.deploy_and_transfer_hashes() {
            if !self.buffer.contains_key(hash) {
                self.buffer.insert(*hash, (expiry_timestamp, None));
            }
            self.dead.insert(*hash);
        }
        // deploys held for proposed blocks which did not get finalized in time are eligible again
        let (hold, _) = mem::take(&mut self.hold)
            .into_iter()
            .partition(|(ts, _)| *ts > timestamp);
        debug!(%timestamp, "DeployBuffer: timestamp finalized");
        self.hold = hold;
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
        let mut holds = vec![];
        for (with_approvals, footprint) in self.proposable() {
            let deploy_hash = *with_approvals.deploy_hash();
            match ret.add(with_approvals, &footprint) {
                Ok(_) => {
                    debug!(%deploy_hash, "DeployBuffer: proposing deploy");
                    holds.push(deploy_hash);
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
                            continue;
                        }
                        AddError::InvalidDeploy => {
                            // it should not be possible for an invalid deploy to get buffered
                            // in the first place, thus this should be unreachable
                            error!(
                                ?deploy_hash,
                                "DeployBuffer: invalid deploy in deploy buffer"
                            );
                            self.dead.insert(deploy_hash);
                            continue;
                        }
                        AddError::TransferCount
                        | AddError::DeployCount
                        | AddError::ApprovalCount
                        | AddError::GasLimit
                        | AddError::BlockSize => {
                            debug!(
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

        // put a hold on all proposed deploys / transfers
        self.hold.insert(timestamp, holds.iter().copied().collect());
        ret
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
        match (&self.status, event) {
            (ComponentStatus::Fatal(msg), _) => {
                warn!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            (ComponentStatus::Uninitialized, evt) => {
                if let Event::Initialize(blocks) = evt {
                    for block in blocks {
                        self.register_block_finalized(&block.into());
                    }
                    self.status = ComponentStatus::Initialized;
                    // start self-expiry management on initialization
                    effect_builder
                        .set_timeout(self.cfg.expiry_check_interval().into())
                        .event(move |_| Event::Expire)
                } else {
                    warn!("should not handle this event when component is uninitialized");
                    Effects::new()
                }
            }
            (ComponentStatus::Initialized, evt) => match evt {
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
                    self.register_block_finalized(&FinalizedBlock::from(*block));
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
