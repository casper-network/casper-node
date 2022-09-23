mod config;
mod event;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem,
};

use datasize::DataSize;
use tracing::{debug, error, warn};

use casper_types::{bytesrepr::ToBytes, Timestamp};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        Component, ComponentStatus, InitializedComponent,
    },
    effect::{EffectBuilder, EffectExt, Effects},
    types::{
        appendable_block::{AddError, AppendableBlock},
        chainspec::DeployConfig,
        Deploy, DeployFootprint, DeployHash, DeployWithApprovals, FinalizedBlock,
    },
    NodeRng,
};
pub(crate) use config::Config;
use event::DeployBufferRequest;
pub(crate) use event::Event;

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
            .for_each(|(_, v)| v.retain(|v| !freed.contains_key(v)));
        self.hold.retain(|_, v| !v.is_empty());

        self.dead.retain(|v| !freed.contains_key(v));
        self.buffer = buffer;
    }

    fn register_deploy(&mut self, deploy: Deploy) {
        let deploy_hash = deploy.id();
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

    fn block_proposed(&mut self, proposed_block: ProposedBlock<ClContext>) {
        if let Some(hold_set) = self.hold.get_mut(&proposed_block.context().timestamp()) {
            hold_set.extend(
                proposed_block
                    .value()
                    .deploy_hashes()
                    .chain(proposed_block.value().transfer_hashes()),
            )
        }
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
            .filter(|(k, _)| !self.hold.values().any(|hs| hs.contains(k)))
            .filter(|(k, _)| !self.dead.contains(k))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn appendable_block(&mut self, timestamp: Timestamp) -> AppendableBlock {
        let mut ret = AppendableBlock::new(self.deploy_config, timestamp);
        let mut holds = vec![];
        for deploy in self.proposable() {
            let deploy_hash = *deploy.id();
            let footprint = match deploy.footprint() {
                Ok(deploy_footprint) => deploy_footprint,
                Err(_) => {
                    error!(%deploy_hash, "invalid deploy in the proposable set");
                    self.dead.insert(deploy_hash);
                    continue;
                }
            };
            let with_approvals = DeployWithApprovals::new(deploy_hash, deploy.approvals().clone());
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
                        | AddError::BlockSize
                        | AddError::InvalidGasAmount => {
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
                    .set_timeout(self.cfg.expiry())
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
                Event::Request(DeployBufferRequest::GetAppendableBlock(timestamp, responder)),
            ) => responder.respond(self.appendable_block(timestamp)).ignore(),
            (ComponentStatus::Initialized, Event::BlockFinalized(finalized_block)) => {
                self.block_finalized(&*finalized_block);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::BlockProposed(proposed)) => {
                self.block_proposed(*proposed);
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::ReceiveDeploy(deploy)) => {
                self.register_deploy(*deploy);
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
