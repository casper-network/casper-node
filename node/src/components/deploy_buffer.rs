use std::{
    collections::{BTreeMap, HashSet},
    mem,
};

use casper_types::Timestamp;
use tracing::{debug, error};

use crate::{
    components::{
        block_proposer::DeployInfo,
        consensus::{ClContext, ProposedBlock},
        Component, ComponentStatus, InitializedComponent,
    },
    effect::{
        announcements::BlockProposerAnnouncement, requests::StateStoreRequest, EffectBuilder,
        Effects,
    },
    storage::StorageRequest,
    types::{chainspec::DeployConfig, Block, Deploy, DeployHash, FinalizedBlock},
    NodeRng,
};

// enum DeployEligibility {
//     Proposable,
//     Unproposable,
//     Proposed,
// }

pub(crate) enum DeployBufferRequest {
    GetDeploys, // TODO
}

pub(crate) enum Event {
    Request(DeployBufferRequest),
    Initialize,            // do we need to be initialized?
    ReceiveDeploy(Deploy), // post Deploy Acceptor
    BlockProposed(Box<ProposedBlock<ClContext>>),
    BlockFinalized(Box<FinalizedBlock>), // be told about blocks to cross off deploys in buffer AND expire dead deploys
}

struct Config {
    deploy_config: DeployConfig,
}

struct DeployBuffer {
    deploy_config: DeployConfig,
    buffer: BTreeMap<DeployHash, DeployInfo>,
    proposable: Vec<DeployHash>,
    hold: BTreeMap<Timestamp, HashSet<DeployHash>>, // timestamp is proposed block timestamp
    dead: HashSet<DeployHash>,
}

impl DeployBuffer {
    pub(crate) fn new(cfg: Config) -> Self {
        DeployBuffer {
            deploy_config: cfg.deploy_config,
            buffer: BTreeMap::new(),
            proposable: vec![],
            hold: BTreeMap::new(),
            dead: HashSet::new(),
        }
    }

    fn register(&mut self, deploy_hash: DeployHash, deploy_info: DeployInfo) {
        // TODO: decide if the debug prints should be warn or error
        // TODO: decide if we should communicate success / failure
        if self.dead.contains(&deploy_hash) {
            debug!(?deploy_hash, "attempt to register already dead deploy");
            return;
        }
        if self.hold.values().any(|dhs| dhs.contains(&deploy_hash)) {
            debug!(?deploy_hash, "attempt to register already held deploy");
            return;
        }
        if self.buffer.insert(deploy_hash, deploy_info).is_none() {
            self.proposable.push(deploy_hash);
        }
    }

    fn block_proposed(&mut self, proposed_block: &ProposedBlock<ClContext>) {
        todo!()
        // self.hold
        //     .entry(proposed_block.context().timestamp())
        //     .or_default()
        //     .extend(
        //         proposed_block
        //             .value()
        //             .deploys()
        //             .iter()
        //             .chain(proposed_block.value().transactions())
        //             .map(),
        //     );
    }

    fn block_finalized(&mut self, finalized_block: &FinalizedBlock) {
        self.dead.extend(
            finalized_block
                .deploy_hashes()
                .iter()
                .chain(finalized_block.transfer_hashes())
                .copied(),
        );
        let (hold, freed) = mem::take(&mut self.hold)
            .into_iter()
            .partition(|(timestamp, _)| *timestamp > finalized_block.timestamp());
        self.hold = hold;
        // Orphaned proposals go to the front of the proposable list:
        let old_proposable = mem::take(&mut self.proposable);
        let proposable = freed
            .into_values()
            .flatten()
            .filter(|deploy_hash| {
                !self.dead.contains(&deploy_hash)
                    | self.hold.values().any(|dhs| dhs.contains(&deploy_hash))
            })
            .chain(old_proposable)
            .collect();
        self.proposable = proposable;
    }
}

impl<REv> Component<REv> for DeployBuffer
where
    REv: From<Event>
        + From<StorageRequest>
        + From<StateStoreRequest> // TODO: Can we NOT do this?
        + From<BlockProposerAnnouncement>
        + Send
        + 'static,
{
    type Event = Event;
    type ConstructionError = ();

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::BlockFinalized(finalized_block) => {
                self.block_finalized(&*finalized_block);
                Effects::new()
            }
            Event::BlockProposed(proposed) => {
                // TODO: squat on set of deploy hashes
                Effects::new()
            }
            Event::Request(_) => {
                // TODO: Return an Arc<BlockPayload> with the requested context and a set of
                // deploys and transfers that are not currently in `hold` or `dead`, ideally
                // in FIFO order, i.e. those that we received first.
                Effects::new()
            }
            Event::Initialize => todo!(),
            Event::ReceiveDeploy(deploy) => {
                match deploy.deploy_info() {
                    Ok(deploy_info) => self.register(*deploy.id(), deploy_info),
                    Err(err) => error!(?err, "tried to register an invalid deploy"),
                }
                Effects::new()
            }
        }
    }
}
