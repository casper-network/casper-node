mod config;
mod event;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem,
};

use datasize::DataSize;
use tracing::{debug, error};

use casper_types::{bytesrepr::ToBytes, Timestamp};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        Component, ComponentStatus, InitializedComponent,
    },
    effect::{EffectBuilder, EffectExt, Effects},
    types::{chainspec::DeployConfig, Deploy, DeployHash, FinalizedBlock},
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;
use event::{DeployBufferRequest, ProposableDeploy};

#[derive(DataSize, Debug)]
struct DeployBuffer {
    status: ComponentStatus,
    cfg: Config,
    deploy_config: DeployConfig,
    buffer: HashMap<DeployHash, Deploy>,
    hold: BTreeMap<Timestamp, HashSet<DeployHash>>,
    dead: HashSet<DeployHash>,
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
        }
    }

    fn expire(&mut self) {
        let earliest_acceptable_timestamp = Timestamp::now() - self.deploy_config.max_ttl;
        let (buffer, freed): (HashMap<_, _>, _) = mem::take(&mut self.buffer)
            .into_iter()
            .partition(|(_, v)| v.header().timestamp() >= earliest_acceptable_timestamp);

        // clear expired deploy from all holds, then clear any entries that have no items remaining
        self.hold
            .iter_mut()
            .for_each(|(_, v)| v.retain(|v| freed.contains_key(v) == false));
        self.hold.retain(|_, v| v.is_empty() == false);

        self.dead.retain(|v| freed.contains_key(v) == false);
        self.buffer = buffer;
    }

    fn register_deploy(&mut self, deploy_hash: DeployHash, deploy: Deploy) {
        if self.dead.contains(&deploy_hash) {
            debug!(?deploy_hash, "attempt to register already dead deploy");
            return;
        }
        if self.hold.values().any(|dhs| dhs.contains(&deploy_hash)) {
            debug!(?deploy_hash, "attempt to register already held deploy");
            return;
        }
        self.buffer.insert(deploy_hash, deploy);
    }

    fn block_proposed(&mut self, proposed_block: Box<ProposedBlock<ClContext>>) {
        self.hold
            .get_mut(&proposed_block.context().timestamp())
            .map(|hs| {
                hs.extend(
                    proposed_block
                        .value()
                        .deploy_hashes()
                        .chain(proposed_block.value().transfer_hashes()),
                )
            });
    }

    fn block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        // all deploys in the finalized block must not be included in future proposals
        self.dead.extend(
            finalized_block
                .deploy_hashes()
                .iter()
                .chain(finalized_block.transfer_hashes())
                .copied(),
        );
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
            .filter(|(k, _)| self.hold.values().any(|hs| hs.contains(k)) == false)
            .filter(|(k, _)| self.dead.contains(k) == false)
            .map(|(_, v)| v.clone())
            .collect()
    }

    //fn proposable_deploys(&mut self, timestamp: Timestamp) -> BlockPayload {
    fn proposable_deploys(&mut self, timestamp: Timestamp) -> Vec<ProposableDeploy> {
        fn skip(current: &mut usize, next: usize, max: usize) -> bool {
            if *current + next > max {
                return true;
            }
            *current += next;
            return false;
        }

        let max_block_size = self.deploy_config.max_block_size as usize;
        let max_gas_limit = self.deploy_config.block_gas_limit as usize;
        let max_approvals = self.deploy_config.block_max_approval_count as usize;
        let max_transfers = self.deploy_config.block_max_transfer_count as usize;
        let max_non_transfers = self.deploy_config.block_max_deploy_count as usize;

        let mut total_size = 0;
        let mut total_gas = 0;
        let mut total_approvals = 0;
        let mut transfers_count = 0;
        let mut non_transfers_count = 0;

        let mut ret = vec![];
        for deploy in self.proposable() {
            if total_size >= max_block_size
                || total_gas >= max_gas_limit
                || total_approvals >= max_approvals
                || (transfers_count >= max_transfers && non_transfers_count >= max_non_transfers)
            {
                break;
            }

            let deploy_hash = *deploy.id();
            if deploy.header().expired(timestamp) {
                self.dead.insert(deploy_hash);
                continue;
            }
            if skip(
                &mut total_approvals,
                deploy.approvals().len(),
                max_approvals,
            ) {
                continue;
            }
            if skip(&mut total_size, deploy.serialized_length(), max_block_size) {
                continue;
            }

            match deploy.payment().payment_amount(deploy.header().gas_price()) {
                Some(gas) => {
                    if skip(&mut total_gas, gas.value().as_usize(), max_gas_limit) {
                        continue;
                    }
                }
                None => {
                    self.dead.insert(*deploy.id());
                    continue;
                }
            }

            let is_transfer = deploy.session().is_transfer();
            if is_transfer {
                if skip(&mut transfers_count, 1, max_transfers) {
                    continue;
                }
                ret.push(ProposableDeploy::Transfer(deploy));
            } else {
                if skip(&mut non_transfers_count, 1, max_non_transfers) {
                    continue;
                }
                ret.push(ProposableDeploy::Deploy(deploy));
            }
        }

        // put a hold on all proposed deploys / transfers
        self.hold
            .insert(timestamp, ret.iter().map(|pd| pd.deploy_hash()).collect());

        ret
    }
}

impl<REv> InitializedComponent<REv> for DeployBuffer
where
    REv: From<Event> + Send + 'static,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

impl<REv> Component<REv> for DeployBuffer
where
    REv: From<Event> + Send + 'static,
{
    type Event = Event;
    type ConstructionError = ();

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match (self.status.clone(), event) {
            (ComponentStatus::Fatal(msg), _) => {
                error!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                return Effects::new();
            }
            (ComponentStatus::Uninitialized, Event::Initialize) => {
                self.status = ComponentStatus::Initialized;
                // start self-expiry management on initialization
                effect_builder
                    .set_timeout(self.cfg.expiry())
                    .event(move |_| Event::Expire)
            }
            (ComponentStatus::Uninitialized, _) => {
                error!("should not handle this event when component is uninitialized");
                self.status =
                    ComponentStatus::Fatal("attempt to use uninitialized component".to_string());
                return Effects::new();
            }
            (ComponentStatus::Initialized, Event::Initialize) => {
                error!("should not initialize when component is already initialized");
                self.status =
                    ComponentStatus::Fatal("attempt to reinitialize component".to_string());
                return Effects::new();
            }
            (
                ComponentStatus::Initialized,
                Event::Request(DeployBufferRequest::GetProposableDeploys(timestamp, responder)),
            ) => responder
                .respond(self.proposable_deploys(timestamp))
                .ignore(),
            (ComponentStatus::Initialized, Event::BlockFinalized(finalized_block)) => {
                self.block_finalized(&*finalized_block);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::BlockProposed(proposed)) => {
                self.block_proposed(proposed);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::ReceiveDeploy(deploy)) => {
                self.register_deploy(*deploy.id(), deploy);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::Expire) => {
                self.expire();
                effect_builder
                    .set_timeout(self.cfg.expiry())
                    .event(move |_| Event::Expire)
            }
        }
    }
}
