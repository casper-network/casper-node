//! Main reactor for nodes.

mod config;
mod error;
mod event;
mod fetchers;
mod memory_metrics;
#[cfg(test)]
mod tests;
mod utils;

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use casper_execution_engine::core::{
    engine_state,
    engine_state::{ChainspecRegistry, UpgradeConfig, UpgradeSuccess},
};
use casper_types::{Key, StoredValue, TimeDiff, Timestamp};
use datasize::DataSize;
use itertools::Itertools;
use prometheus::Registry;
use tracing::error;

use crate::{
    components::{
        block_proposer::{self, BlockProposer},
        block_synchronizer::{self, BlockSynchronizer},
        block_validator::{self, BlockValidator},
        blocks_accumulator::{BlocksAccumulator, LeapInstruction, StartingWith},
        chain_synchronizer::{self, ChainSynchronizer},
        consensus::{self, EraSupervisor, HighwayProtocol},
        contract_runtime::ContractRuntime,
        deploy_acceptor::{self, DeployAcceptor},
        diagnostics_port::{self, DiagnosticsPort},
        event_stream_server::{self, EventStreamServer},
        gossiper::{self, Gossiper},
        linear_chain::{self, LinearChainComponent},
        metrics::Metrics,
        rest_server,
        rest_server::RestServer,
        rpc_server,
        rpc_server::RpcServer,
        small_network::{self, GossipedAddress, Identity as NetworkIdentity, SmallNetwork},
        storage::{FatalStorageError, Storage},
        sync_leaper,
        sync_leaper::SyncLeaper,
        upgrade_watcher::{self, UpgradeWatcher},
        Component,
    },
    effect::{
        announcements::{
            BlockProposerAnnouncement, BlocklistAnnouncement, ChainSynchronizerAnnouncement,
            ConsensusAnnouncement, ContractRuntimeAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, LinearChainAnnouncement, RpcServerAnnouncement,
            UpgradeWatcherAnnouncement,
        },
        incoming::{BlockAddedResponseIncoming, NetResponseIncoming, TrieResponseIncoming},
        requests::ChainspecRawBytesRequest,
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    reactor::{
        self,
        event_queue_metrics::EventQueueMetrics,
        main_reactor::{
            fetchers::Fetchers,
            utils::{initialize_component, maybe_upgrade},
        },
        EventQueueHandle, ReactorExit,
    },
    types::{
        ActivationPoint, Block, BlockAdded, BlockHash, Chainspec, ChainspecRawBytes, Deploy,
        ExitCode, FinalitySignature, Item, NodeId, SyncLeap, TrieOrChunk,
    },
    utils::{Source, WithDir},
    NodeRng,
};
#[cfg(test)]
use crate::{testing::network::NetworkedReactor, types::NodeId};
pub(crate) use config::Config;
pub(crate) use error::Error;
pub(crate) use event::MainEvent;
use memory_metrics::MemoryMetrics;

#[derive(DataSize, Debug)]
enum ReactorState {
    Initialize,
    CatchUp,
    KeepUp,
}

/// Main node reactor.
#[derive(DataSize, Debug)]
pub(crate) struct MainReactor {
    trusted_hash: Option<BlockHash>,
    storage: Storage,
    contract_runtime: ContractRuntime, // TODO: handle the `set_initial_state`
    upgrade_watcher: UpgradeWatcher,

    small_network: SmallNetwork<MainEvent, Message>, /* TODO: handle setting the
                                                      * `is_syncing_peer` - needs
                                                      * internal init state */
    // TODO - has its own timing belt - should it?
    address_gossiper: Gossiper<GossipedAddress, MainEvent>,

    rpc_server: RpcServer, /* TODO: make sure the handling in "Initialize & CatchUp" phase is
                            * correct (explicit error messages, etc.) - needs an init event? */
    rest_server: RestServer,
    event_stream_server: EventStreamServer,
    deploy_acceptor: DeployAcceptor, /* TODO: should use
                                      * `get_highest_COMPLETE_block_header_from_storage()` */

    fetchers: Fetchers,

    deploy_gossiper: Gossiper<Deploy, MainEvent>,
    block_added_gossiper: Gossiper<BlockAdded, MainEvent>,
    finality_signature_gossiper: Gossiper<FinalitySignature, MainEvent>,

    sync_leaper: SyncLeaper,
    block_proposer: BlockProposer, // TODO: handle providing highest block, etc.
    consensus: EraSupervisor,      /* TODO: Update constructor (provide less state) and extend
                                    * handler for the "block added" ann. */
    block_validator: BlockValidator,
    linear_chain: LinearChainComponent, // TODO: Maybe redundant.
    chain_synchronizer: ChainSynchronizer<MainEvent>, // TODO: To be removed.
    blocks_accumulator: BlocksAccumulator,
    block_synchronizer: BlockSynchronizer,

    // Non-components.
    diagnostics_port: DiagnosticsPort,
    metrics: Metrics,
    #[data_size(skip)] // Never allocates heap data.
    memory_metrics: MemoryMetrics,
    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,
    state: ReactorState,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,

    max_attempts: usize,
    attempts: usize,
    idle_tolerances: TimeDiff,
}

impl MainReactor {
    fn check_status(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Effects<MainEvent> {
        const WAIT_SEC: u64 = 15; // TODO: config setting this
        let effects = Effects::new();
        match self.state {
            ReactorState::Initialize => {
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.diagnostics_port,
                    "diagnotics".to_string(),
                    MainEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.upgrade_watcher,
                    "upgrade_watcher".to_string(),
                    MainEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.event_stream_server,
                    "event_stream_server".to_string(),
                    MainEvent::EventStreamServer(event_stream_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.rest_server,
                    "rest_server".to_string(),
                    MainEvent::RestServer(rest_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.rpc_server,
                    "rpc_server".to_string(),
                    MainEvent::RpcServer(rpc_server::Event::Initialize),
                ) {
                    return effects;
                }
                self.state = ReactorState::CatchUp;
            }
            ReactorState::CatchUp => {
                let mut effects = Effects::new();
                if let Some(timestamp) = self.block_synchronizer.last_progress() {
                    if Timestamp::now().saturating_diff(timestamp) <= self.idle_tolerances {
                        self.attempts = 0; // if any progress has been made, reset attempts
                        effects.extend(
                            effect_builder
                                .set_timeout(Duration::from_secs(WAIT_SEC))
                                .event(|_| MainEvent::CheckStatus),
                        );
                        return effects;
                    } else {
                        self.attempts += 1;
                        if self.attempts > self.max_attempts {
                            effects.extend(effect_builder.immediately().event(|()| {
                                MainEvent::Shutdown(
                                    "catch up process exceeds idle tolerances".to_string(),
                                )
                            }));
                            return effects;
                        }
                    }
                }

                // check optional config trusted hash && optional local tip
                /*
                    ++ : self.storage.get_block(config.trusted_hash).height >< tip.height
                    +- : leap w/ config hash
                    -+ : leap w/ local tip hash
                    -- : check pre-genesis and apply or if post-genesis shutdown
                */
                let starting_with = {
                    if let Some(trusted_hash) = self.trusted_hash {
                        match self.storage.read_block(&trusted_hash) {
                            Ok(Some(trusted_block)) => {
                                match self.linear_chain.highest_block() {
                                    Some(block) => {
                                        StartingWith::Block(Box::new(block.clone()))
                                        // may want to compare heights
                                    }
                                    None => {
                                        // should be unreachable
                                        StartingWith::Hash(trusted_hash)
                                    }
                                }
                            }
                            Ok(None) => StartingWith::Hash(trusted_hash),
                            Err(_) => {
                                effects.extend(effect_builder.immediately().event(move |_| {
                                    MainEvent::Shutdown("fatal block store error".to_string())
                                }));
                                return effects;
                            }
                        }
                    } else {
                        match self.linear_chain.highest_block() {
                            Some(block) => StartingWith::Block(Box::new(block.clone())),
                            None => {
                                if let ActivationPoint::Genesis(timestamp) =
                                    self.chainspec.protocol_config.activation_point
                                {
                                    // push apply genesis effect
                                    // TODO: wire up genesis (can't run test network without
                                    // genesis)
                                    effects.extend(
                                        effect_builder
                                            .immediately()
                                            .event(|()| MainEvent::CheckStatus),
                                    );
                                    return effects;
                                } else {
                                    effects.extend(effect_builder.immediately().event(move |_| {
                                        MainEvent::Shutdown("fatal block store error".to_string())
                                    }));
                                    return effects;
                                }
                            }
                        }
                    }
                };

                let trusted_hash = *starting_with.block_hash();
                match self.blocks_accumulator.should_leap(starting_with) {
                    LeapInstruction::Leap => {
                        let peers_to_ask = self
                            .small_network
                            .peers_random(
                                rng,
                                self.chainspec
                                    .core_config
                                    .sync_leap_simultaneous_peer_requests,
                            )
                            .into_keys()
                            .into_iter()
                            .collect_vec();
                        effects.extend(effect_builder.immediately().event(move |_| {
                            MainEvent::SyncLeaper(sync_leaper::Event::StartPullingSyncLeap {
                                trusted_hash,
                                peers_to_ask,
                            })
                        }));
                        effects.extend(
                            effect_builder
                                .immediately()
                                .event(|()| MainEvent::CheckStatus),
                        );
                        return effects;
                    }
                    LeapInstruction::CaughtUp => {
                        // TODO: maybe do something w/ the UpgradeWatcher announcement for a
                        // detected upgrade to make this a stronger check
                        match self.linear_chain.highest_block() {
                            Some(block) => {
                                if let Some(upgrade_effects) = maybe_upgrade(
                                    effect_builder,
                                    block,
                                    self.chainspec.clone(),
                                    self.chainspec_raw_bytes.clone(),
                                ) {
                                    effects.extend(upgrade_effects);
                                    return effects;
                                }
                            }
                            None => {
                                // should be unreachable
                                effects.extend(effect_builder.immediately().event(move |_| {
                                    MainEvent::Shutdown(
                                        "can't be caught up with no block in the block store"
                                            .to_string(),
                                    )
                                }));
                                return effects;
                            }
                        }
                    }
                    LeapInstruction::SyncForExec(block_hash) => {
                        // pass block_hash to block_synchronizer
                        todo!()
                    }
                }

                self.state = ReactorState::KeepUp;
            }
            ReactorState::KeepUp => {
                // TODO: if UpgradeWatcher announcement raised, keep track of era id's against the
                // new activation point
                // detected upgrade to make this a stronger check
                // if in validator set and era supervisor is green, validate
                // else get added blocks for block accumulator and execute them
                // if falling behind, switch over to catchup
                // if sync to genesis == true, if cycles available get next historical block
                // try real hard to stay in this mode
                // in case of fkup, shutdown
            }
        }
        effects

        // TODO: Stall detection should possibly be done in the control logic.
    }
}

impl reactor::Reactor for MainReactor {
    type Event = MainEvent;
    type Config = WithDir<Config>;
    type Error = Error;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<MainEvent>), Error> {
        let node_startup_instant = Instant::now();

        let effect_builder = EffectBuilder::new(event_queue);

        let metrics = Metrics::new(registry.clone());
        let memory_metrics = MemoryMetrics::new(registry.clone())?;
        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let protocol_version = chainspec.protocol_config.version;

        let trusted_hash = config.value().node.trusted_hash;

        let (root_dir, config) = config.into_parts();
        let storage_config = WithDir::new(&root_dir, config.storage.clone());

        let hard_reset_to_start_of_era = chainspec.hard_reset_to_start_of_era();
        let storage = Storage::new(
            &storage_config,
            chainspec.highway_config.finality_threshold_fraction,
            hard_reset_to_start_of_era,
            protocol_version,
            &chainspec.network_config.name,
            chainspec.core_config.recent_era_count(),
        )?;

        let contract_runtime = ContractRuntime::new(
            protocol_version,
            storage.root_path(),
            &config.contract_runtime,
            chainspec.wasm_config,
            chainspec.system_costs_config,
            chainspec.core_config.max_associated_keys,
            chainspec.core_config.max_runtime_call_stack_height,
            chainspec.core_config.minimum_delegation_amount,
            chainspec.core_config.strict_argument_checking,
            chainspec.core_config.vesting_schedule_period.millis(),
            registry,
        )?;

        let upgrade_watcher = UpgradeWatcher::new(chainspec.as_ref(), &root_dir)?;

        let node_key_pair = config.consensus.load_keys(&root_dir)?;
        let small_network = SmallNetwork::new(
            config.network.clone(),
            network_identity,
            Some(node_key_pair),
            registry,
            chainspec.as_ref(),
        )?;

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let rpc_server = RpcServer::new(
            config.rpc_server.clone(),
            config.speculative_exec_server.clone(),
            protocol_version,
            chainspec.network_config.name.clone(),
            node_startup_instant,
        );
        let rest_server = RestServer::new(
            config.rest_server.clone(),
            protocol_version,
            chainspec.network_config.name.clone(),
            node_startup_instant,
        );
        let event_stream_server = EventStreamServer::new(
            config.event_stream_server.clone(),
            storage.root_path().to_path_buf(),
            protocol_version,
        );

        let deploy_acceptor = DeployAcceptor::new(chainspec.as_ref(), registry)?;
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, MainEvent>,
            registry,
        )?;
        let block_added_gossiper = Gossiper::new_for_partial_items(
            "block_added_gossiper",
            config.gossip,
            gossiper::get_block_added_from_storage::<BlockAdded, MainEvent>,
            registry,
        )?;
        let finality_signature_gossiper = Gossiper::new_for_partial_items(
            "finality_signature_gossiper",
            config.gossip,
            gossiper::get_finality_signature_from_storage::<FinalitySignature, MainEvent>,
            registry,
        )?;

        let sync_leaper = SyncLeaper::new(chainspec.highway_config.finality_threshold_fraction);

        let block_proposer =
            BlockProposer::new(registry.clone(), &chainspec, config.block_proposer)?;

        let (our_secret_key, our_public_key) = config.consensus.load_keys(&root_dir)?;
        let next_upgrade_activation_point = upgrade_watcher.next_upgrade_activation_point();
        let consensus = EraSupervisor::new(
            storage.root_path(),
            our_secret_key,
            our_public_key,
            config.consensus,
            chainspec.clone(),
            next_upgrade_activation_point,
            registry,
            Box::new(HighwayProtocol::new_boxed),
        )?;

        let block_validator = BlockValidator::new(Arc::clone(&chainspec));
        let linear_chain = LinearChainComponent::new(
            registry,
            protocol_version,
            chainspec.core_config.auction_delay,
            chainspec.core_config.unbonding_delay,
            chainspec.highway_config.finality_threshold_fraction,
            next_upgrade_activation_point,
        )?;

        let (chain_synchronizer, _chain_synchronizer_effects) =
            ChainSynchronizer::<MainEvent>::new_for_sync_to_genesis(
                chainspec.clone(),
                config.node.clone(),
                config.network.clone(),
                chain_synchronizer::Metrics::new(registry).unwrap(),
                effect_builder,
            )?;

        let fetchers = Fetchers::new(&config.fetcher, chainspec.as_ref(), registry)?;

        let blocks_accumulator =
            BlocksAccumulator::new(chainspec.highway_config.finality_threshold_fraction);
        let block_synchronizer = BlockSynchronizer::new(
            config.block_synchronizer,
            chainspec.highway_config.finality_threshold_fraction,
        );

        let diagnostics_port =
            DiagnosticsPort::new(WithDir::new(&root_dir, config.diagnostics_port));

        let reactor = MainReactor {
            chainspec,
            chainspec_raw_bytes,
            storage,
            contract_runtime,
            upgrade_watcher,
            small_network,
            address_gossiper,
            rpc_server,
            rest_server,
            event_stream_server,
            deploy_acceptor,
            fetchers,
            deploy_gossiper,
            block_added_gossiper,
            finality_signature_gossiper,
            sync_leaper,
            block_proposer,
            consensus,
            block_validator,
            linear_chain,
            chain_synchronizer,
            blocks_accumulator,
            block_synchronizer,
            diagnostics_port,
            metrics,
            memory_metrics,
            event_queue_metrics,
            state: ReactorState::Initialize {},
            attempts: 0,
            max_attempts: 3,
            idle_tolerances: TimeDiff::from_seconds(1200),
            trusted_hash,
        };

        let effects = effect_builder
            .immediately()
            .event(|()| MainEvent::CheckStatus);
        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        event: MainEvent,
    ) -> Effects<MainEvent> {
        match event {
            MainEvent::Shutdown(msg) => fatal!(
                effect_builder,
                "reactor should shut down due to error: {}",
                msg,
            )
            .ignore(),
            MainEvent::CheckStatus => self.check_status(effect_builder, rng),
            MainEvent::UpgradeResult(result) => {
                match result {
                    Ok(UpgradeSuccess { .. }) => {
                        // info!(
                        //     network_name = %self.chainspec.network_config.name,
                        //     %post_state_hash,
                        //     "upgrade committed"
                        // );
                        //
                        // let initial_pre_state = ExecutionPreState::new(
                        //     previous_block_header.height() + 1,
                        //     post_state_hash,
                        //     previous_block_header.hash(),
                        //     previous_block_header.accumulated_seed(),
                        // );
                        // let finalized_block = FinalizedBlock::new(
                        //     BlockPayload::default(),
                        //     Some(EraReport::default()),
                        //     previous_block_header.timestamp(),
                        //     previous_block_header.next_block_era_id(),
                        //     initial_pre_state.next_block_height(),
                        //     PublicKey::System,
                        // );
                        //
                        // self.execute_immediate_switch_block(
                        //     effect_builder,
                        //     initial_pre_state,
                        //     finalized_block,
                        // )
                        // TODO: investigate piggy backing
                        // effect_builder.enqueue_block_for_execution
                        // and allowing the contract runtime to handle immediate switch block
                        // like any other finalized block
                        effect_builder
                            .immediately()
                            .event(|()| MainEvent::CheckStatus)
                    }
                    Err(err) => fatal!(
                        effect_builder,
                        "reactor should shut down due to error: {}",
                        err.to_string(),
                    )
                    .ignore(),
                }
            }
            // delegate all fetcher activity to self.fetchers.dispatch_fetcher_event(..)
            MainEvent::DeployFetcher(..)
            | MainEvent::DeployFetcherRequest(..)
            | MainEvent::BlockFetcher(..)
            | MainEvent::BlockFetcherRequest(..)
            | MainEvent::BlockHeaderFetcher(..)
            | MainEvent::BlockHeaderFetcherRequest(..)
            | MainEvent::TrieOrChunkFetcher(..)
            | MainEvent::TrieOrChunkFetcherRequest(..)
            | MainEvent::BlockByHeightFetcher(..)
            | MainEvent::BlockByHeightFetcherRequest(..)
            | MainEvent::BlockHeaderByHeightFetcher(..)
            | MainEvent::BlockHeaderByHeightFetcherRequest(..)
            | MainEvent::BlockAndDeploysFetcher(..)
            | MainEvent::BlockAndDeploysFetcherRequest(..)
            | MainEvent::FinalizedApprovalsFetcher(..)
            | MainEvent::FinalizedApprovalsFetcherRequest(..)
            | MainEvent::BlockHeadersBatchFetcher(..)
            | MainEvent::BlockHeadersBatchFetcherRequest(..)
            | MainEvent::FinalitySignaturesFetcher(..)
            | MainEvent::FinalitySignaturesFetcherRequest(..)
            | MainEvent::SyncLeapFetcher(..)
            | MainEvent::SyncLeapFetcherRequest(..)
            | MainEvent::BlockAddedFetcher(..)
            | MainEvent::BlockAddedFetcherRequest(..)
            | MainEvent::FinalitySignatureFetcher(..)
            | MainEvent::FinalitySignatureFetcherRequest(..) => self
                .fetchers
                .dispatch_fetcher_event(effect_builder, rng, event),
            MainEvent::SyncLeaper(event) => reactor::wrap_effects(
                MainEvent::SyncLeaper,
                self.sync_leaper.handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockProposer(event) => reactor::wrap_effects(
                MainEvent::BlockProposer,
                self.block_proposer.handle_event(effect_builder, rng, event),
            ),
            MainEvent::RpcServer(event) => reactor::wrap_effects(
                MainEvent::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
            MainEvent::Consensus(event) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockValidator(event) => reactor::wrap_effects(
                MainEvent::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::LinearChain(event) => reactor::wrap_effects(
                MainEvent::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlocksAccumulator(event) => reactor::wrap_effects(
                MainEvent::BlocksAccumulator,
                self.blocks_accumulator
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockSynchronizer(event) => reactor::wrap_effects(
                MainEvent::BlockSynchronizer,
                self.block_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockProposerRequest(req) => {
                self.dispatch_event(effect_builder, rng, MainEvent::BlockProposer(req.into()))
            }
            MainEvent::BlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                MainEvent::BlockValidator(block_validator::Event::from(req)),
            ),
            MainEvent::StateStoreRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::BlockSynchronizerRequest(req) => reactor::wrap_effects(
                MainEvent::BlockSynchronizer,
                self.block_synchronizer
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::Client,
                    maybe_responder: responder,
                };
                self.dispatch_event(effect_builder, rng, MainEvent::DeployAcceptor(event))
            }
            MainEvent::ConsensusAnnouncement(consensus_announcement) => {
                match consensus_announcement {
                    ConsensusAnnouncement::Finalized(block) => {
                        let reactor_event =
                            MainEvent::BlockProposer(block_proposer::Event::FinalizedBlock(block));
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::CreatedFinalitySignature(fs) => {
                        let reactor_finality_signatures_gossiper_event =
                            MainEvent::FinalitySignatureGossiper(gossiper::Event::ItemReceived {
                                item_id: fs.id(),
                                source: Source::Ourself,
                            });
                        let mut effects = self.dispatch_event(
                            effect_builder,
                            rng,
                            MainEvent::LinearChain(linear_chain::Event::FinalitySignatureReceived(
                                fs, false,
                            )),
                        );
                        effects.extend(self.dispatch_event(
                            effect_builder,
                            rng,
                            reactor_finality_signatures_gossiper_event,
                        ));
                        effects
                    }
                    ConsensusAnnouncement::Fault {
                        era_id,
                        public_key,
                        timestamp,
                    } => {
                        let reactor_event =
                            MainEvent::EventStreamServer(event_stream_server::Event::Fault {
                                era_id,
                                public_key: *public_key,
                                timestamp,
                            });
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                }
            }
            MainEvent::BlockProposerAnnouncement(BlockProposerAnnouncement::DeploysExpired(
                hashes,
            )) => {
                let reactor_event = MainEvent::EventStreamServer(
                    event_stream_server::Event::DeploysExpired(hashes),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }

            MainEvent::Storage(event) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            MainEvent::SmallNetwork(event) => reactor::wrap_effects(
                MainEvent::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            MainEvent::RestServer(event) => reactor::wrap_effects(
                MainEvent::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            MainEvent::EventStreamServer(event) => reactor::wrap_effects(
                MainEvent::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::UpgradeWatcher(event) => reactor::wrap_effects(
                MainEvent::UpgradeWatcher,
                self.upgrade_watcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DeployAcceptor(event) => reactor::wrap_effects(
                MainEvent::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DeployGossiper(event) => reactor::wrap_effects(
                MainEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockAddedGossiper(event) => reactor::wrap_effects(
                MainEvent::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::FinalitySignatureGossiper(event) => reactor::wrap_effects(
                MainEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::AddressGossiper(event) => reactor::wrap_effects(
                MainEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::ContractRuntimeRequest(req) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::ChainSynchronizer(event) => reactor::wrap_effects(
                MainEvent::ChainSynchronizer,
                self.chain_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::DiagnosticsPort(event) => reactor::wrap_effects(
                MainEvent::DiagnosticsPort,
                self.diagnostics_port
                    .handle_event(effect_builder, rng, event),
            ),
            // Requests:
            MainEvent::ChainSynchronizerRequest(request) => reactor::wrap_effects(
                MainEvent::ChainSynchronizer,
                self.chain_synchronizer
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::NetworkRequest(req) => {
                let event = MainEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            MainEvent::NetworkInfoRequest(req) => {
                let event = MainEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            MainEvent::MetricsRequest(req) => reactor::wrap_effects(
                MainEvent::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            MainEvent::ChainspecRawBytesRequest(
                ChainspecRawBytesRequest::GetChainspecRawBytes(responder),
            ) => responder.respond(self.chainspec_raw_bytes.clone()).ignore(),
            MainEvent::UpgradeWatcherRequest(req) => {
                self.dispatch_event(effect_builder, rng, req.into())
            }
            MainEvent::StorageRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::MarkBlockCompletedRequest(req) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::BeginAddressGossipRequest(req) => reactor::wrap_effects(
                MainEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::DumpConsensusStateRequest(req) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, req.into()),
                // req.answer(Err(Cow::Borrowed("node is joining, no running consensus")))
                //     .ignore()
            ),
            // Announcements:
            MainEvent::ControlAnnouncement(ctrl_ann) => {
                error!("unhandled control announcement: {}", ctrl_ann);
                Effects::new()
            }
            MainEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => {
                let deploy_footprint = match deploy.footprint() {
                    Ok(deploy_footprint) => deploy_footprint,
                    Err(error) => {
                        error!(%error, "invalid deploy");
                        return Effects::new();
                    }
                };

                let mut effects = self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::BlockProposer(block_proposer::Event::BufferDeploy {
                        hash: deploy.deploy_or_transfer_hash(),
                        approvals: deploy.approvals().clone(),
                        footprint: Box::new(deploy_footprint),
                    }),
                );

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployGossiper(gossiper::Event::ItemReceived {
                        item_id: *deploy.id(),
                        source: source.clone(),
                    }),
                ));

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::EventStreamServer(event_stream_server::Event::DeployAccepted(
                        deploy.clone(),
                    )),
                ));

                effects.extend(self.fetchers.dispatch_fetcher_event(
                    effect_builder,
                    rng,
                    MainEvent::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
                    ),
                ));

                effects
            }
            MainEvent::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
            MainEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::LinearChainBlock {
                    block,
                    approvals_checksum,
                    execution_results_checksum,
                    execution_results,
                },
            ) => {
                let mut effects = Effects::new();
                let block_hash = *block.hash();

                // send to linear chain
                let reactor_event =
                    MainEvent::LinearChain(linear_chain::Event::NewLinearChainBlock {
                        block,
                        approvals_checksum,
                        execution_results_checksum,
                        execution_results: execution_results
                            .iter()
                            .map(|(hash, _header, results)| (*hash, results.clone()))
                            .collect(),
                    });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));

                // send to event stream
                for (deploy_hash, deploy_header, execution_result) in execution_results {
                    let reactor_event =
                        MainEvent::EventStreamServer(event_stream_server::Event::DeployProcessed {
                            deploy_hash,
                            deploy_header: Box::new(deploy_header),
                            block_hash,
                            execution_result: Box::new(execution_result),
                        });
                    effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                }

                effects
            }
            MainEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::CommitStepSuccess {
                    era_id,
                    execution_effect,
                },
            ) => {
                let reactor_event =
                    MainEvent::EventStreamServer(event_stream_server::Event::Step {
                        era_id,
                        execution_effect,
                    });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            MainEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::UpcomingEraValidators {
                    era_that_is_ending,
                    upcoming_era_validators,
                },
            ) => {
                let mut events = self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::BlockSynchronizer(block_synchronizer::Event::EraValidators {
                        validators: upcoming_era_validators.clone(),
                    }),
                );
                events.extend(
                    self.dispatch_event(
                        effect_builder,
                        rng,
                        MainEvent::SmallNetwork(
                            ContractRuntimeAnnouncement::UpcomingEraValidators {
                                era_that_is_ending,
                                upcoming_era_validators,
                            }
                            .into(),
                        ),
                    ),
                );
                events
            }
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_deploy_id,
            )) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new deploy");
                Effects::new()
            }
            MainEvent::DeployGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _gossiped_deploy_id,
            )) => {
                // let reactor_event =
                //     MainEvent::BlockProposer(block_proposer::Event::
                // BufferDeploy(gossiped_deploy_id));
                // self.dispatch_event(effect_builder, rng, reactor_event)
                Effects::new()
            }
            MainEvent::BlockAddedGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_block_added_id,
            )) => {
                error!(%gossiped_block_added_id, "gossiper should not announce new block-added");
                Effects::new()
            }
            MainEvent::BlockAddedGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _gossiped_block_added_id,
            )) => Effects::new(),
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_finality_signature_id),
            ) => {
                error!(%gossiped_finality_signature_id, "gossiper should not announce new finality signature");
                Effects::new()
            }
            MainEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_gossiped_finality_signature_id),
            ) => Effects::new(),
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_address,
            )) => {
                let reactor_event = MainEvent::SmallNetwork(
                    small_network::Event::PeerAddressReceived(gossiped_address),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            MainEvent::AddressGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_)) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }
            MainEvent::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded {
                block,
                approvals_checksum,
                execution_results_checksum,
            }) => {
                let reactor_event_consensus = MainEvent::Consensus(consensus::Event::BlockAdded {
                    header: Box::new(block.header().clone()),
                    header_hash: *block.hash(),
                    approvals_checksum,
                    execution_results_checksum,
                });
                let reactor_block_gossiper_event =
                    MainEvent::BlockAddedGossiper(gossiper::Event::ItemReceived {
                        item_id: *block.hash(),
                        source: Source::Ourself,
                    });
                let reactor_event_es =
                    MainEvent::EventStreamServer(event_stream_server::Event::BlockAdded(block));
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event_es);
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_consensus));
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    reactor_block_gossiper_event,
                ));

                effects
            }
            MainEvent::LinearChainAnnouncement(LinearChainAnnouncement::NewFinalitySignature(
                fs,
            )) => {
                let reactor_event =
                    MainEvent::EventStreamServer(event_stream_server::Event::FinalitySignature(fs));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            MainEvent::ChainSynchronizerAnnouncement(
                ChainSynchronizerAnnouncement::SyncFinished,
            ) => self.dispatch_event(
                effect_builder,
                rng,
                MainEvent::SmallNetwork(small_network::Event::ChainSynchronizerAnnouncement(
                    ChainSynchronizerAnnouncement::SyncFinished,
                )),
            ),
            MainEvent::UpgradeWatcherAnnouncement(
                UpgradeWatcherAnnouncement::UpgradeActivationPointRead(next_upgrade),
            ) => {
                let reactor_event = MainEvent::UpgradeWatcher(
                    upgrade_watcher::Event::GotNextUpgrade(next_upgrade.clone()),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event);

                let reactor_event = MainEvent::Consensus(
                    consensus::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                let reactor_event = MainEvent::LinearChain(
                    linear_chain::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                effects
            }
            MainEvent::BlocklistAnnouncement(ann) => {
                let mut effects = Effects::new();
                match &ann {
                    BlocklistAnnouncement::OffenseCommitted(node_id) => {
                        let event = MainEvent::BlockSynchronizer(
                            block_synchronizer::Event::DisconnectFromPeer(**node_id),
                        );
                        effects.extend(self.dispatch_event(effect_builder, rng, event));
                    }
                }
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    MainEvent::SmallNetwork(ann.into()),
                ));
                effects
            }
            MainEvent::ConsensusMessageIncoming(incoming) => reactor::wrap_effects(
                MainEvent::Consensus,
                self.consensus
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::BlockAddedGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::FinalitySignatureGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::AddressGossiperIncoming(incoming) => reactor::wrap_effects(
                MainEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::NetRequestIncoming(incoming) => reactor::wrap_effects(
                MainEvent::Storage,
                self.storage
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            MainEvent::NetResponseIncoming(NetResponseIncoming { sender, message }) => {
                reactor::handle_get_response(self, effect_builder, rng, sender, message)
            }
            MainEvent::TrieRequestIncoming(req) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            MainEvent::TrieDemand(demand) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, demand.into()),
            ),
            MainEvent::TrieResponseIncoming(TrieResponseIncoming { sender, message }) => {
                reactor::handle_fetch_response::<Self, TrieOrChunk>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                )
            }
            MainEvent::BlockAddedRequestIncoming(_req) => {
                // if this gets routed to storage, we can remove this BlockAddedRequestIncoming
                // variant and just use the NetRequestIncoming
                //    OR
                // route to blocks-accumulator? to construct and send the network response
                todo!()
            }
            MainEvent::BlockAddedResponseIncoming(BlockAddedResponseIncoming {
                sender,
                message,
            }) => {
                // route to blocks-accumulator and announce once validated to route to gossiper
                // AND
                // route to fetcher
                reactor::handle_fetch_response::<Self, BlockAdded>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                )
            }

            MainEvent::FinalitySignatureIncoming(incoming) => {
                todo!(); // route it to both the LinearChain and BlocksAccumulator
                reactor::wrap_effects(
                    MainEvent::LinearChain,
                    self.linear_chain
                        .handle_event(effect_builder, rng, incoming.into()),
                )
            }
            MainEvent::ContractRuntime(event) => reactor::wrap_effects(
                MainEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
        }
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.memory_metrics.estimate(self);
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle)
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.linear_chain
            .stop_for_upgrade()
            .then(|| ReactorExit::ProcessShouldExit(ExitCode::Success))
    }
}

#[cfg(test)]
impl MainReactor {
    pub(crate) fn consensus(&self) -> &EraSupervisor {
        &self.consensus
    }

    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    pub(crate) fn contract_runtime(&self) -> &ContractRuntime {
        &self.contract_runtime
    }
}

#[cfg(test)]
impl NetworkedReactor for MainReactor {
    fn node_id(&self) -> NodeId {
        self.small_network.node_id()
    }
}
