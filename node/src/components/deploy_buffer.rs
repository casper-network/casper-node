mod config;
mod event;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem,
};

use datasize::DataSize;
use tracing::{debug, error, warn};

use casper_types::Timestamp;

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        Component, ComponentStatus, InitializedComponent,
    },
    effect::{
        announcements::DeployBufferAnnouncement, requests::DeployBufferRequest, EffectBuilder,
        EffectExt, Effects,
    },
    types::{
        appendable_block::{AddError, AppendableBlock},
        chainspec::DeployConfig,
        Deploy, DeployHash, DeployHashWithApprovals, FinalizedBlock,
    },
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;

#[derive(DataSize, Debug)]
pub(crate) struct DeployBuffer {
    status: ComponentStatus,
    cfg: Config,
    deploy_config: DeployConfig,
    // keeps track of all deploys the buffer is currently aware of
    // hold and dead are used to filter it on demand as necessary
    // items are removed via a self-perpetuating expire event
    buffer: HashMap<DeployHash, Deploy>,
    // when a maybe-block is in flight, we pause inclusion
    // of the deploys within it in other proposed blocks
    // if the maybe-block becomes an actual block the
    // deploy hashes will get put to self.dead
    // otherwise, the hold will be released and the deploys
    // will become eligible to propose again.
    hold: BTreeMap<Timestamp, HashSet<DeployHash>>,
    // deploy_hashes that should not be proposed, ever
    dead: HashSet<DeployHash>,
    // block_height and block_time of blocks added to the local chain
    // needed to ensure we have seen sufficient blocks
    // to allow consensus to proceed safely without risking
    // duplicating deploys
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

    // do you have full TTL worth of deploy awareness
    pub(crate) fn have_full_ttl_of_deploys(&self, from_height: u64) -> bool {
        let ttl = self.deploy_config.max_ttl;
        let mut curr = match self.chain_index.get(&from_height) {
            None => {
                return false;
            }
            Some(timestamp) => (from_height, timestamp),
        };

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

    fn expire<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<Event> + From<DeployBufferAnnouncement> + Send,
    {
        let earliest_acceptable_timestamp = Timestamp::now() - self.deploy_config.max_ttl;
        let (buffer, freed): (HashMap<_, _>, _) = mem::take(&mut self.buffer)
            .into_iter()
            .partition(|(_, deploy)| deploy.header().timestamp() >= earliest_acceptable_timestamp);

        // clear expired deploy from all holds, then clear any entries that have no items remaining
        self.hold.iter_mut().for_each(|(_, held_deploys)| {
            held_deploys.retain(|deploy_hash| !freed.contains_key(deploy_hash))
        });
        self.hold.retain(|_, v| !v.is_empty());

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

    fn register_deploy(&mut self, deploy: Deploy) {
        let deploy_hash = deploy.hash();
        if deploy.is_valid().is_err() {
            error!(?deploy_hash, "invalid deploy must not be buffered");
            return;
        }
        if self.dead.contains(deploy_hash) {
            debug!(?deploy_hash, "attempt to register already dead deploy");
            return;
        }
        if self.hold.values().any(|dhs| dhs.contains(deploy_hash)) {
            debug!(?deploy_hash, "attempt to register already held deploy");
            return;
        }
        self.buffer.insert(*deploy_hash, deploy);
    }

    fn register_block_proposed(&mut self, proposed_block: ProposedBlock<ClContext>) {
        if let Some(hold_set) = self.hold.get_mut(&proposed_block.context().timestamp()) {
            hold_set.extend(
                proposed_block
                    .value()
                    .deploy_hashes()
                    .chain(proposed_block.value().transfer_hashes()),
            )
        }
    }

    fn register_block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        self.chain_index
            .insert(finalized_block.height(), finalized_block.timestamp());
        // all deploys in the finalized block must not be included in future proposals
        self.dead
            .extend(finalized_block.deploy_hashes().iter().copied());
        self.dead
            .extend(finalized_block.transfer_hashes().iter().copied());
        // deploys held for proposed blocks which did not get finalized in time are eligible again
        let (hold, _) = mem::take(&mut self.hold)
            .into_iter()
            .partition(|(timestamp, _)| *timestamp > finalized_block.timestamp());
        self.hold = hold;
    }

    fn proposable(&self) -> Vec<Deploy> {
        // a deploy hash that is not in dead or hold is proposable
        self.buffer
            .iter()
            .filter(|(k, _)| !self.hold.values().any(|hs| hs.contains(k)))
            .filter(|(k, _)| !self.dead.contains(k))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn appendable_block(&mut self, timestamp: Timestamp) -> AppendableBlock {
        let mut ret = AppendableBlock::new(self.deploy_config, timestamp);
        let mut holds = vec![];
        for deploy in self.proposable() {
            let deploy_hash = *deploy.hash();
            let footprint = match deploy.footprint() {
                Ok(deploy_footprint) => deploy_footprint,
                Err(_) => {
                    error!(%deploy_hash, "invalid deploy in the proposable set");
                    self.dead.insert(deploy_hash);
                    continue;
                }
            };
            let with_approvals =
                DeployHashWithApprovals::new(deploy_hash, deploy.approvals().clone());
            match ret.add(with_approvals, &footprint) {
                Ok(_) => {
                    holds.push(deploy_hash);
                }
                Err(error) => {
                    match error {
                        AddError::Duplicate => {
                            // it should be physically impossible for a duplicate deploy to
                            // be in the deploy buffer, thus this should be unreachable
                            debug!(?deploy_hash, "duplicated deploy in deploy buffer");
                            self.dead.insert(deploy_hash);
                            continue;
                        }
                        AddError::InvalidDeploy => {
                            // it should not be possible for an invalid deploy to get buffered
                            // in the first place, thus this should be unreachable
                            debug!(?deploy_hash, "invalid deploy in deploy buffer");
                            self.dead.insert(deploy_hash);
                            continue;
                        }
                        AddError::TransferCount
                        | AddError::DeployCount
                        | AddError::ApprovalCount
                        | AddError::GasLimit
                        | AddError::BlockSize => {
                            // one or more block limits have been reached
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
    REv: From<Event> + From<DeployBufferAnnouncement> + Send + 'static,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

impl<REv> Component<REv> for DeployBuffer
where
    REv: From<Event> + From<DeployBufferAnnouncement> + Send + 'static,
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
                error!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            (ComponentStatus::Uninitialized, Event::Initialize) => {
                self.status = ComponentStatus::Initialized;
                // start self-expiry management on initialization
                effect_builder
                    .set_timeout(self.cfg.expiry_check_interval().into())
                    .event(move |_| Event::Expire)
            }
            (ComponentStatus::Uninitialized, _) => {
                warn!("should not handle this event when component is uninitialized");
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::Initialize) => {
                // noop
                Effects::new()
            }
            (
                ComponentStatus::Initialized,
                Event::Request(DeployBufferRequest::GetAppendableBlock {
                    timestamp,
                    responder,
                }),
            ) => responder.respond(self.appendable_block(timestamp)).ignore(),
            (ComponentStatus::Initialized, Event::BlockFinalized(finalized_block)) => {
                self.register_block_finalized(&*finalized_block);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::Block(block)) => {
                self.register_block_finalized(&FinalizedBlock::from(*block));
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::BlockProposed(proposed)) => {
                self.register_block_proposed(*proposed);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::ReceiveDeploy(deploy)) => {
                self.register_deploy(*deploy);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::Expire) => self.expire(effect_builder),
        }
    }
}
