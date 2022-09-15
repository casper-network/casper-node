//! Reactor for participating nodes.
//!
//! Participating nodes join the participating-only network upon startup.

mod config;
mod error;
mod event;
mod fetchers;
mod memory_metrics;
#[cfg(test)]
mod tests;
mod utils;

use std::{sync::Arc, time::Instant};

use datasize::DataSize;
use prometheus::Registry;
use tracing::error;

use crate::{
    components::{
        block_proposer::{self, BlockProposer},
        block_validator::{self, BlockValidator},
        blocks_accumulator::BlocksAccumulator,
        chain_synchronizer::{self, ChainSynchronizer},
        complete_block_synchronizer::{self, CompleteBlockSynchronizer},
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
        small_network::{self, GossipedAddress, SmallNetwork},
        storage::Storage,
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
        incoming::{
            BlockAddedResponseIncoming, NetResponseIncoming, SyncLeapResponseIncoming,
            TrieResponseIncoming,
        },
        requests::ChainspecRawBytesRequest,
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    reactor::{
        self,
        event_queue_metrics::EventQueueMetrics,
        participating::{fetchers::Fetchers, utils::initialize_component},
        EventQueueHandle, ReactorExit,
    },
    types::{
        BlockAdded, Chainspec, ChainspecRawBytes, Deploy, ExitCode, FinalitySignature, Item,
        SyncLeap, TrieOrChunk,
    },
    utils::{Source, WithDir},
    NodeRng,
};
#[cfg(test)]
use crate::{testing::network::NetworkedReactor, types::NodeId};
pub(crate) use config::Config;
pub(crate) use error::Error;
pub(crate) use event::ParticipatingEvent;
use memory_metrics::MemoryMetrics;

#[derive(DataSize, Debug)]
enum ReactorState {
    Initialize,
    CatchUp,
    KeepUp,
}

/// Participating node reactor.
#[derive(DataSize, Debug)]
pub(crate) struct Reactor {
    storage: Storage,
    contract_runtime: ContractRuntime, // TODO: handle the `set_initial_state`
    upgrade_watcher: UpgradeWatcher,

    small_network: SmallNetwork<ParticipatingEvent, Message>, /* TODO: handle setting the
                                                               * `is_syncing_peer` - needs
                                                               * internal init state */
    // TODO - has its own timing belt - should it?
    address_gossiper: Gossiper<GossipedAddress, ParticipatingEvent>,

    rpc_server: RpcServer, /* TODO: make sure the handling in "Initialize & CatchUp" phase is
                            * correct (explicit error messages, etc.) - needs an init event? */
    rest_server: RestServer,
    event_stream_server: EventStreamServer,
    deploy_acceptor: DeployAcceptor, /* TODO: should use
                                      * `get_highest_COMPLETE_block_header_from_storage()` */

    fetchers: Fetchers,

    deploy_gossiper: Gossiper<Deploy, ParticipatingEvent>,
    block_added_gossiper: Gossiper<BlockAdded, ParticipatingEvent>,
    finality_signature_gossiper: Gossiper<FinalitySignature, ParticipatingEvent>,

    block_proposer: BlockProposer, // TODO: handle providing highest block, etc.
    consensus: EraSupervisor,      /* TODO: Update constructor (provide less state) and extend
                                    * handler for the "block added" ann. */
    block_validator: BlockValidator,
    linear_chain: LinearChainComponent, // TODO: Maybe redundant.
    chain_synchronizer: ChainSynchronizer<ParticipatingEvent>, // TODO: To be removed.
    blocks_accumulator: BlocksAccumulator,
    complete_block_synchronizer: CompleteBlockSynchronizer,

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
}

impl Reactor {
    fn check_status(
        &mut self,
        effect_builder: EffectBuilder<ParticipatingEvent>,
        _rng: &mut NodeRng,
    ) -> Effects<ParticipatingEvent> {
        let effects = Effects::new();
        match self.state {
            ReactorState::Initialize => {
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.diagnostics_port,
                    "diagnotics".to_string(),
                    ParticipatingEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.upgrade_watcher,
                    "upgrade_watcher".to_string(),
                    ParticipatingEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.event_stream_server,
                    "event_stream_server".to_string(),
                    ParticipatingEvent::EventStreamServer(event_stream_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.rest_server,
                    "rest_server".to_string(),
                    ParticipatingEvent::RestServer(rest_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.rpc_server,
                    "rpc_server".to_string(),
                    ParticipatingEvent::RpcServer(rpc_server::Event::Initialize),
                ) {
                    return effects;
                }
                // .. and so on
            }
            ReactorState::CatchUp => {
                // check block accumulator
                // leap, switch to keep up, or shut down
                // if no progress is being made, reattempt config'd # of times then shutdown
                // idleness should be same as consensus
                // any progress at all will touch the idleness counter (keeping it alive longer)
            }
            ReactorState::KeepUp => {
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

impl reactor::Reactor for Reactor {
    type Event = ParticipatingEvent;
    type Config = WithDir<Config>;
    type Error = Error;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<ParticipatingEvent>), Error> {
        let node_startup_instant = Instant::now();

        let effect_builder = EffectBuilder::new(event_queue);

        let metrics = Metrics::new(registry.clone());
        let memory_metrics = MemoryMetrics::new(registry.clone())?;
        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let protocol_version = chainspec.protocol_config.version;

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
        // contract_runtime.set_initial_state(ExecutionPreState::from_block_header(
        //     todo!(), //&highest_block_header
        // ))?;

        let upgrade_watcher = UpgradeWatcher::new(chainspec.as_ref(), &root_dir)?;

        let node_key_pair = config.consensus.load_keys(&root_dir)?;
        let small_network = SmallNetwork::new(
            config.network.clone(),
            Some(node_key_pair),
            registry,
            chainspec.as_ref(),
        )?;

        // let ParticipatingInitConfig {
        //     root,
        //     config,
        //     storage,
        //     mut contract_runtime,
        //     //joining_outcome,
        //     //chain_sync_metrics,
        //     //event_stream_server,
        //     small_network_identity,
        //     node_startup_instant,
        // } = config;

        // TODO: Check if we should do any of this things in different place now.
        // info!(?joining_outcome, "handling joining outcome");
        // let highest_block_header = match joining_outcome {
        //     JoiningOutcome::ShouldExitForUpgrade => {
        //         error!("invalid joining outcome to transition to participating reactor");
        //         return Err(Error::InvalidJoiningOutcome);
        //     }
        //     JoiningOutcome::Synced {
        //         highest_block_header,
        //     } => {
        //         if let Some(BlockAndExecutionEffects {
        //             block,
        //             execution_results,
        //             maybe_step_effect_and_upcoming_era_validators,
        //         }) = chainspec
        //             .maybe_immediate_switch_block_data()
        //             .cloned()
        //         {
        //             // The outcome of joining in this case caused a new switch block to be
        // created,             // so we need to emit the effects which would have been
        // created by that             // execution, but add them to the participating
        // reactor's event queues so they             // don't get dropped as the joining
        // reactor shuts down.             effects.extend(
        //                 effect_builder
        //                     .announce_new_linear_chain_block(block.clone(), execution_results)
        //                     .ignore(),
        //             );

        //             let current_era_id = block.header().era_id();
        //             if let Some(step_effect_and_upcoming_era_validators) =
        //                 maybe_step_effect_and_upcoming_era_validators
        //             {
        //                 effects.extend(
        //                     effect_builder
        //                         .announce_commit_step_success(
        //                             current_era_id,
        //
        // step_effect_and_upcoming_era_validators.step_execution_journal,
        // )                         .ignore(),
        //                 );
        //                 effects.extend(
        //                     effect_builder
        //                         .announce_upcoming_era_validators(
        //                             current_era_id,
        //
        // step_effect_and_upcoming_era_validators.upcoming_era_validators,
        // )                         .ignore(),
        //                 );
        //             }

        //             let secret_key = our_secret_key.clone();
        //             let public_key = our_public_key.clone();
        //             let block_hash = *block.hash();
        //             effects.extend(
        //                 async move {
        //                     let validator_weights =
        //                         match linear_chain::era_validator_weights_for_block(
        //                             block.header(),
        //                             effect_builder,
        //                         )
        //                         .await
        //                         {
        //                             Ok((_era_id, weights)) => weights,
        //                             Err(error) => {
        //                                 return fatal!(
        //                                     effect_builder,
        //                                     "couldn't get era validators for header: {}",
        //                                     error
        //                                 )
        //                                 .await;
        //                             }
        //                         };

        //                     // We're responsible for signing the new block if we're in the
        // provided                     // list.
        //                     if validator_weights.contains_key(&public_key) {
        //                         let signature = FinalitySignature::create(
        //                             block_hash,
        //                             current_era_id,
        //                             &secret_key,
        //                             public_key.clone(),
        //                         );

        //                         effect_builder
        //                             .announce_created_finality_signature(signature.clone())
        //                             .await;
        //                         // Allow a short period for peers to establish connections. This
        //                         // delay can be removed once we move to a single reactor model.
        //                         effect_builder
        //                             .set_timeout(DELAY_FOR_SIGNING_IMMEDIATE_SWITCH_BLOCK)
        //                             .await;
        //                         let message = Message::FinalitySignature(Box::new(signature));
        //                         effect_builder
        //                             .broadcast_message_to_validators(message, current_era_id)
        //                             .await;
        //                     }
        //                 }
        //                 .ignore(),
        //             );
        //         }

        //         *highest_block_header
        //     }
        // };

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
            gossiper::get_deploy_from_storage::<Deploy, ParticipatingEvent>,
            registry,
        )?;
        let block_added_gossiper = Gossiper::new_for_partial_items(
            "block_added_gossiper",
            config.gossip,
            gossiper::get_block_added_from_storage::<BlockAdded, ParticipatingEvent>,
            registry,
        )?;
        let finality_signature_gossiper = Gossiper::new_for_partial_items(
            "finality_signature_gossiper",
            config.gossip,
            gossiper::get_finality_signature_from_storage::<FinalitySignature, ParticipatingEvent>,
            registry,
        )?;

        let block_proposer = BlockProposer::new(
            registry.clone(),
            //highest_block_header.height() + 1,
            &chainspec,
            config.block_proposer,
        )?;

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
            ChainSynchronizer::<ParticipatingEvent>::new_for_sync_to_genesis(
                chainspec.clone(),
                config.node.clone(),
                config.network.clone(),
                chain_synchronizer::Metrics::new(registry).unwrap(),
                effect_builder,
            )?;

        let fetchers = Fetchers::new(&config.fetcher, chainspec.as_ref(), registry)?;

        let blocks_accumulator =
            BlocksAccumulator::new(chainspec.highway_config.finality_threshold_fraction);
        let complete_block_synchronizer = CompleteBlockSynchronizer::new(
            config.complete_block_synchronizer,
            chainspec.highway_config.finality_threshold_fraction,
        );

        let diagnostics_port =
            DiagnosticsPort::new(WithDir::new(&root_dir, config.diagnostics_port.clone()));

        let reactor = Reactor {
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
            block_proposer,
            consensus,
            block_validator,
            linear_chain,
            chain_synchronizer,
            blocks_accumulator,
            complete_block_synchronizer,
            diagnostics_port,
            metrics,
            memory_metrics,
            event_queue_metrics,
            state: ReactorState::Initialize {},
        };

        let effects = effect_builder
            .immediately()
            .event(|()| ParticipatingEvent::CheckStatus);
        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<ParticipatingEvent>,
        rng: &mut NodeRng,
        event: ParticipatingEvent,
    ) -> Effects<ParticipatingEvent> {
        match event {
            ParticipatingEvent::Shutdown(msg) => {
                return fatal!(
                    effect_builder,
                    "reactor should shut down due to error: {}",
                    msg,
                )
                .ignore();
                //kill me now please
            }
            ParticipatingEvent::CheckStatus => self.check_status(effect_builder, rng),
            // delegate all fetcher activity to self.fetchers.dispatch_fetcher_event(..)
            ParticipatingEvent::DeployFetcher(..)
            | ParticipatingEvent::DeployFetcherRequest(..)
            | ParticipatingEvent::BlockFetcher(..)
            | ParticipatingEvent::BlockFetcherRequest(..)
            | ParticipatingEvent::BlockHeaderFetcher(..)
            | ParticipatingEvent::BlockHeaderFetcherRequest(..)
            | ParticipatingEvent::TrieOrChunkFetcher(..)
            | ParticipatingEvent::TrieOrChunkFetcherRequest(..)
            | ParticipatingEvent::BlockByHeightFetcher(..)
            | ParticipatingEvent::BlockByHeightFetcherRequest(..)
            | ParticipatingEvent::BlockHeaderByHeightFetcher(..)
            | ParticipatingEvent::BlockHeaderByHeightFetcherRequest(..)
            | ParticipatingEvent::BlockAndDeploysFetcher(..)
            | ParticipatingEvent::BlockAndDeploysFetcherRequest(..)
            | ParticipatingEvent::FinalizedApprovalsFetcher(..)
            | ParticipatingEvent::FinalizedApprovalsFetcherRequest(..)
            | ParticipatingEvent::BlockHeadersBatchFetcher(..)
            | ParticipatingEvent::BlockHeadersBatchFetcherRequest(..)
            | ParticipatingEvent::FinalitySignaturesFetcher(..)
            | ParticipatingEvent::FinalitySignaturesFetcherRequest(..)
            | ParticipatingEvent::SyncLeapFetcher(..)
            | ParticipatingEvent::SyncLeapFetcherRequest(..)
            | ParticipatingEvent::BlockAddedFetcher(..)
            | ParticipatingEvent::BlockAddedFetcherRequest(..)
            | ParticipatingEvent::FinalitySignatureFetcher(..)
            | ParticipatingEvent::FinalitySignatureFetcherRequest(..) => self
                .fetchers
                .dispatch_fetcher_event(effect_builder, rng, event),

            // Participating only
            ParticipatingEvent::BlockProposer(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockProposer,
                self.block_proposer.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::RpcServer(event) => reactor::wrap_effects(
                ParticipatingEvent::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::Consensus(event) => reactor::wrap_effects(
                ParticipatingEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockValidator(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::LinearChain(event) => reactor::wrap_effects(
                ParticipatingEvent::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlocksAccumulator(event) => reactor::wrap_effects(
                ParticipatingEvent::BlocksAccumulator,
                self.blocks_accumulator
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::CompleteBlockSynchronizer(event) => reactor::wrap_effects(
                ParticipatingEvent::CompleteBlockSynchronizer,
                self.complete_block_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockProposerRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::BlockProposer(req.into()),
            ),
            ParticipatingEvent::BlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::BlockValidator(block_validator::Event::from(req)),
            ),
            ParticipatingEvent::StateStoreRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::CompleteBlockSynchronizerRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::CompleteBlockSynchronizer,
                self.complete_block_synchronizer
                    .handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::Client,
                    maybe_responder: responder,
                };
                self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::DeployAcceptor(event),
                )
            }
            ParticipatingEvent::ConsensusAnnouncement(consensus_announcement) => {
                match consensus_announcement {
                    ConsensusAnnouncement::Finalized(block) => {
                        let reactor_event = ParticipatingEvent::BlockProposer(
                            block_proposer::Event::FinalizedBlock(block),
                        );
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::CreatedFinalitySignature(fs) => {
                        let reactor_finality_signatures_gossiper_event =
                            ParticipatingEvent::FinalitySignatureGossiper(
                                gossiper::Event::ItemReceived {
                                    item_id: fs.id(),
                                    source: Source::Ourself,
                                },
                            );
                        let mut effects = self.dispatch_event(
                            effect_builder,
                            rng,
                            ParticipatingEvent::LinearChain(
                                linear_chain::Event::FinalitySignatureReceived(fs, false),
                            ),
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
                        let reactor_event = ParticipatingEvent::EventStreamServer(
                            event_stream_server::Event::Fault {
                                era_id,
                                public_key: *public_key,
                                timestamp,
                            },
                        );
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                }
            }
            ParticipatingEvent::BlockProposerAnnouncement(
                BlockProposerAnnouncement::DeploysExpired(hashes),
            ) => {
                let reactor_event = ParticipatingEvent::EventStreamServer(
                    event_stream_server::Event::DeploysExpired(hashes),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }

            // Common for participating and joiner
            ParticipatingEvent::Storage(event) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::SmallNetwork(event) => reactor::wrap_effects(
                ParticipatingEvent::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::RestServer(event) => reactor::wrap_effects(
                ParticipatingEvent::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::EventStreamServer(event) => reactor::wrap_effects(
                ParticipatingEvent::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::UpgradeWatcher(event) => reactor::wrap_effects(
                ParticipatingEvent::UpgradeWatcher,
                self.upgrade_watcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DeployAcceptor(event) => reactor::wrap_effects(
                ParticipatingEvent::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DeployGossiper(event) => reactor::wrap_effects(
                ParticipatingEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockAddedGossiper(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::FinalitySignatureGossiper(event) => reactor::wrap_effects(
                ParticipatingEvent::FinalitySignatureGossiper,
                self.finality_signature_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::AddressGossiper(event) => reactor::wrap_effects(
                ParticipatingEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::ContractRuntimeRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::ChainSynchronizer(event) => reactor::wrap_effects(
                ParticipatingEvent::ChainSynchronizer,
                self.chain_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DiagnosticsPort(event) => reactor::wrap_effects(
                ParticipatingEvent::DiagnosticsPort,
                self.diagnostics_port
                    .handle_event(effect_builder, rng, event),
            ),
            // Requests:
            ParticipatingEvent::ChainSynchronizerRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::ChainSynchronizer,
                self.chain_synchronizer
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::NetworkRequest(req) => {
                let event = ParticipatingEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            ParticipatingEvent::NetworkInfoRequest(req) => {
                let event = ParticipatingEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            ParticipatingEvent::MetricsRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            ParticipatingEvent::ChainspecRawBytesRequest(
                ChainspecRawBytesRequest::GetChainspecRawBytes(responder),
            ) => responder.respond(self.chainspec_raw_bytes.clone()).ignore(),
            ParticipatingEvent::UpgradeWatcherRequest(req) => {
                self.dispatch_event(effect_builder, rng, req.into())
            }
            ParticipatingEvent::StorageRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::MarkBlockCompletedRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::BeginAddressGossipRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::DumpConsensusStateRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, req.into()),
                // req.answer(Err(Cow::Borrowed("node is joining, no running consensus")))
                //     .ignore()
            ),
            // Announcements:
            ParticipatingEvent::ControlAnnouncement(ctrl_ann) => {
                error!("unhandled control announcement: {}", ctrl_ann);
                Effects::new()
            }
            ParticipatingEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => {
                let deploy_info = match deploy.deploy_info() {
                    Ok(deploy_info) => deploy_info,
                    Err(error) => {
                        error!(%error, "invalid deploy");
                        return Effects::new();
                    }
                };

                let mut effects = self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::BlockProposer(block_proposer::Event::BufferDeploy {
                        hash: deploy.deploy_or_transfer_hash(),
                        approvals: deploy.approvals().clone(),
                        deploy_info: Box::new(deploy_info),
                    }),
                );

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::DeployGossiper(gossiper::Event::ItemReceived {
                        item_id: *deploy.id(),
                        source: source.clone(),
                    }),
                ));

                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::EventStreamServer(
                        event_stream_server::Event::DeployAccepted(deploy.clone()),
                    ),
                ));

                effects.extend(self.fetchers.dispatch_fetcher_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
                    ),
                ));

                effects
            }
            ParticipatingEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::InvalidDeploy {
                    deploy: _,
                    source: _,
                },
            ) => Effects::new(),
            ParticipatingEvent::ContractRuntimeAnnouncement(
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
                    ParticipatingEvent::LinearChain(linear_chain::Event::NewLinearChainBlock {
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
                    let reactor_event = ParticipatingEvent::EventStreamServer(
                        event_stream_server::Event::DeployProcessed {
                            deploy_hash,
                            deploy_header: Box::new(deploy_header),
                            block_hash,
                            execution_result: Box::new(execution_result),
                        },
                    );
                    effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                }

                effects
            }
            ParticipatingEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::CommitStepSuccess {
                    era_id,
                    execution_effect,
                },
            ) => {
                let reactor_event =
                    ParticipatingEvent::EventStreamServer(event_stream_server::Event::Step {
                        era_id,
                        execution_effect,
                    });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::UpcomingEraValidators {
                    era_that_is_ending,
                    upcoming_era_validators,
                },
            ) => {
                let mut events = self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::CompleteBlockSynchronizer(
                        complete_block_synchronizer::Event::EraValidators {
                            validators: upcoming_era_validators.clone(),
                        },
                    ),
                );
                events.extend(
                    self.dispatch_event(
                        effect_builder,
                        rng,
                        ParticipatingEvent::SmallNetwork(
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
            ParticipatingEvent::DeployGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_deploy_id),
            ) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new deploy");
                Effects::new()
            }
            ParticipatingEvent::DeployGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_gossiped_deploy_id),
            ) => {
                // let reactor_event =
                //     ParticipatingEvent::BlockProposer(block_proposer::Event::
                // BufferDeploy(gossiped_deploy_id));
                // self.dispatch_event(effect_builder, rng, reactor_event)
                Effects::new()
            }
            ParticipatingEvent::BlockAddedGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_block_added_id),
            ) => {
                error!(%gossiped_block_added_id, "gossiper should not announce new block-added");
                Effects::new()
            }
            ParticipatingEvent::BlockAddedGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_gossiped_block_added_id),
            ) => Effects::new(),
            ParticipatingEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_finality_signature_id),
            ) => {
                error!(%gossiped_finality_signature_id, "gossiper should not announce new finality signature");
                Effects::new()
            }
            ParticipatingEvent::FinalitySignatureGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_gossiped_finality_signature_id),
            ) => Effects::new(),
            ParticipatingEvent::AddressGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_address),
            ) => {
                let reactor_event = ParticipatingEvent::SmallNetwork(
                    small_network::Event::PeerAddressReceived(gossiped_address),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::AddressGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_),
            ) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }
            ParticipatingEvent::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded {
                block,
                approvals_checksum,
                execution_results_checksum,
            }) => {
                let reactor_event_consensus =
                    ParticipatingEvent::Consensus(consensus::Event::BlockAdded {
                        header: Box::new(block.header().clone()),
                        header_hash: *block.hash(),
                        approvals_checksum,
                        execution_results_checksum,
                    });
                let reactor_block_gossiper_event =
                    ParticipatingEvent::BlockAddedGossiper(gossiper::Event::ItemReceived {
                        item_id: *block.hash(),
                        source: Source::Ourself,
                    });
                let reactor_event_es = ParticipatingEvent::EventStreamServer(
                    event_stream_server::Event::BlockAdded(block),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event_es);
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_consensus));
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    reactor_block_gossiper_event,
                ));

                effects
            }
            ParticipatingEvent::LinearChainAnnouncement(
                LinearChainAnnouncement::NewFinalitySignature(fs),
            ) => {
                let reactor_event = ParticipatingEvent::EventStreamServer(
                    event_stream_server::Event::FinalitySignature(fs),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::ChainSynchronizerAnnouncement(
                ChainSynchronizerAnnouncement::SyncFinished,
            ) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::SmallNetwork(
                    small_network::Event::ChainSynchronizerAnnouncement(
                        ChainSynchronizerAnnouncement::SyncFinished,
                    ),
                ),
            ),
            ParticipatingEvent::UpgradeWatcherAnnouncement(
                UpgradeWatcherAnnouncement::UpgradeActivationPointRead(next_upgrade),
            ) => {
                let reactor_event = ParticipatingEvent::UpgradeWatcher(
                    upgrade_watcher::Event::GotNextUpgrade(next_upgrade.clone()),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event);

                let reactor_event = ParticipatingEvent::Consensus(
                    consensus::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                let reactor_event = ParticipatingEvent::LinearChain(
                    linear_chain::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                effects
            }
            ParticipatingEvent::BlocklistAnnouncement(ann) => {
                let mut effects = Effects::new();
                match &ann {
                    BlocklistAnnouncement::OffenseCommitted(node_id) => {
                        let event = ParticipatingEvent::CompleteBlockSynchronizer(
                            complete_block_synchronizer::Event::DisconnectFromPeer(**node_id),
                        );
                        effects.extend(self.dispatch_event(effect_builder, rng, event));
                    }
                }
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::SmallNetwork(ann.into()),
                ));
                effects
            }
            ParticipatingEvent::ConsensusMessageIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::Consensus,
                self.consensus
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::BlockAddedGossiperIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::FinalitySignatureGossiperIncoming(incoming) => {
                reactor::wrap_effects(
                    ParticipatingEvent::FinalitySignatureGossiper,
                    self.finality_signature_gossiper.handle_event(
                        effect_builder,
                        rng,
                        incoming.into(),
                    ),
                )
            }
            ParticipatingEvent::AddressGossiperIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::NetRequestIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::NetResponseIncoming(NetResponseIncoming { sender, message }) => {
                reactor::handle_get_response(self, effect_builder, rng, sender, message)
            }
            ParticipatingEvent::TrieRequestIncoming(req) => reactor::wrap_effects(
                ParticipatingEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::TrieDemand(demand) => reactor::wrap_effects(
                ParticipatingEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, demand.into()),
            ),
            ParticipatingEvent::TrieResponseIncoming(TrieResponseIncoming { sender, message }) => {
                reactor::handle_fetch_response::<Self, TrieOrChunk>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                )
            }
            ParticipatingEvent::SyncLeapRequestIncoming(_req) => {
                // if this gets routed to storage, we can remove this SyncLeapRequestIncoming
                // variant and just use the NetRequestIncoming
                //    OR
                // route to SyncLeaper once it's implemented
                todo!()
            }
            ParticipatingEvent::SyncLeapResponseIncoming(SyncLeapResponseIncoming {
                sender,
                message,
            }) => reactor::handle_fetch_response::<Self, SyncLeap>(
                self,
                effect_builder,
                rng,
                sender,
                &message.0,
            ),
            ParticipatingEvent::BlockAddedRequestIncoming(_req) => {
                // if this gets routed to storage, we can remove this BlockAddedRequestIncoming
                // variant and just use the NetRequestIncoming
                //    OR
                // route to blocks-accumulator? to construct and send the network response
                todo!()
            }
            ParticipatingEvent::BlockAddedResponseIncoming(BlockAddedResponseIncoming {
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

            ParticipatingEvent::FinalitySignatureIncoming(incoming) => {
                todo!(); // route it to both the LinearChain and BlocksAccumulator
                reactor::wrap_effects(
                    ParticipatingEvent::LinearChain,
                    self.linear_chain
                        .handle_event(effect_builder, rng, incoming.into()),
                )
            }
            ParticipatingEvent::ContractRuntime(event) => reactor::wrap_effects(
                ParticipatingEvent::ContractRuntime,
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
impl Reactor {
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
impl NetworkedReactor for Reactor {
    fn node_id(&self) -> NodeId {
        self.small_network.node_id()
    }
}
