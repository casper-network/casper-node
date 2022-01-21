mod error;
mod event;
mod operations;

use std::{convert::Infallible, fmt::Debug, sync::Arc};

use datasize::DataSize;
use tracing::{debug, error, info, warn};

use casper_execution_engine::core::engine_state::{
    self, genesis::GenesisSuccess, UpgradeConfig, UpgradeSuccess,
};
use casper_types::{EraId, PublicKey};

use crate::{
    components::{
        consensus::EraReport,
        contract_runtime::{BlockAndExecutionEffects, BlockExecutionError, ExecutionPreState},
        Component,
    },
    effect::{EffectBuilder, EffectExt, Effects},
    fatal,
    reactor::joiner::JoinerEvent,
    types::{
        ActivationPoint, BlockHash, BlockHeader, BlockPayload, Chainspec, FinalizedBlock,
        NodeConfig, Timestamp,
    },
    NodeRng,
};
use error::Error;
pub(crate) use event::Event;
pub(crate) use operations::KeyBlockInfo;

#[derive(DataSize, Debug)]
pub(crate) enum JoiningOutcome {
    /// We need to shutdown for upgrade as we downloaded a block from a higher protocol version.
    ShouldExitForUpgrade,
    /// We finished synchronizing, with the given block header being the result of the sync task.
    Synced { latest_block_header: BlockHeader },
    /// We didn't sync, but ran `commit_genesis` or `commit_upgrade` and created the given switch
    /// block immediately afterwards.
    RanUpgradeOrGenesis {
        block_and_execution_effects: BlockAndExecutionEffects,
    },
}

#[derive(DataSize, Debug)]
pub(crate) struct ChainSynchronizer {
    chainspec: Arc<Chainspec>,
    config: NodeConfig,
    /// This will be populated once the synchronizer has completed all work, indicating the joiner
    /// reactor can stop running.  It is passed to the participating reactor's constructor via its
    /// config.
    joining_outcome: Option<JoiningOutcome>,
    /// The next upgrade activation point, used to determine what action to take after completing
    /// chain synchronization.
    maybe_next_upgrade: Option<ActivationPoint>,
}

impl ChainSynchronizer {
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        config: NodeConfig,
        maybe_next_upgrade: Option<ActivationPoint>,
        effect_builder: EffectBuilder<JoinerEvent>,
    ) -> (Self, Effects<Event>) {
        let synchronizer = ChainSynchronizer {
            chainspec,
            config,
            joining_outcome: None,
            maybe_next_upgrade,
        };
        let effects = match synchronizer.config.trusted_hash.as_ref() {
            None => {
                // If no trusted hash was provided in the config, get the highest block from storage
                // in order to use its hash, or in the case of no blocks, to commit genesis.
                effect_builder
                    .get_highest_block_from_storage()
                    .event(|maybe_highest_block| {
                        Event::HighestBlockHash(maybe_highest_block.map(|block| *block.hash()))
                    })
            }
            Some(trusted_hash) => synchronizer.start_syncing(effect_builder, *trusted_hash),
        };
        (synchronizer, effects)
    }

    pub(crate) fn joining_outcome(&self) -> Option<&JoiningOutcome> {
        self.joining_outcome.as_ref()
    }

    pub(crate) fn into_joining_outcome(self) -> Option<JoiningOutcome> {
        self.joining_outcome
    }

    fn start_syncing(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        trusted_hash: BlockHash,
    ) -> Effects<Event> {
        info!(%trusted_hash, "synchronizing linear chain");
        operations::run_chain_sync_task(
            effect_builder,
            trusted_hash,
            Arc::clone(&self.chainspec),
            self.config.clone(),
        )
        .event(Event::SyncResult)
    }

    fn handle_sync_result(
        &mut self,
        effect_builder: EffectBuilder<JoinerEvent>,
        result: Result<BlockHeader, Error>,
    ) -> Effects<Event> {
        match result {
            Ok(latest_block_header) => {
                if latest_block_header.protocol_version() == self.chainspec.protocol_version() {
                    if let Some(next_upgrade_activation_point) = self
                        .maybe_next_upgrade
                        .map(|next_upgrade| next_upgrade.era_id())
                    {
                        if latest_block_header.era_id() >= next_upgrade_activation_point {
                            // This is an invalid run as the highest block era ID >= next activation
                            // point, so we're running an outdated version.  Exit with success to
                            // indicate we should upgrade.
                            warn!(
                                %next_upgrade_activation_point,
                                %latest_block_header,
                                "running outdated version: exit to upgrade"
                            );
                            self.joining_outcome = Some(JoiningOutcome::ShouldExitForUpgrade);
                            return Effects::new();
                        }
                    }
                    self.joining_outcome = Some(JoiningOutcome::Synced {
                        latest_block_header,
                    });
                    Effects::new()
                } else if self
                    .chainspec
                    .protocol_config
                    .is_last_block_before_activation(&latest_block_header)
                {
                    self.commit_upgrade(effect_builder, latest_block_header)
                } else {
                    error!(
                        ?latest_block_header,
                        "failed to sync linear chain: unexpected latest block header: not our \
                         version and not the previous upgrade point"
                    );
                    fatal!(
                        effect_builder,
                        "unexpected latest block header after syncing"
                    )
                    .ignore()
                }
            }
            Err(Error::RetrievedBlockHeaderFromFutureVersion {
                current_version,
                block_header_with_future_version,
            }) => {
                let future_version = block_header_with_future_version.protocol_version();
                info!(%current_version, %future_version, "shutting down for upgrade");
                self.joining_outcome = Some(JoiningOutcome::ShouldExitForUpgrade);
                Effects::new()
            }
            Err(error) => {
                error!(%error, "failed to sync linear chain");
                fatal!(effect_builder, "{}", error).ignore()
            }
        }
    }

    fn handle_highest_block(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        maybe_highest_block: Option<BlockHash>,
    ) -> Effects<Event> {
        // If we have a block in storage, use its hash as the trusted hash to sync to.  If not,
        // since the user provided no trusted hash, we can only continue if this is an initial run
        // at network genesis.
        match maybe_highest_block {
            Some(trusted_hash) => self.start_syncing(effect_builder, trusted_hash),
            None => self.commit_genesis(effect_builder),
        }
    }

    fn commit_genesis(&self, effect_builder: EffectBuilder<JoinerEvent>) -> Effects<Event> {
        let genesis_timestamp = match self.genesis_timestamp() {
            None => {
                return fatal!(
                    effect_builder,
                    "node started with no trusted hash, no stored blocks, and no genesis timestamp \
                    in chainspec - specify a trusted hash and restart"
                )
                .ignore();
            }
            Some(timestamp) => timestamp,
        };

        let now = Timestamp::now();
        let era_duration = self.chainspec.core_config.era_duration;
        if now > genesis_timestamp + era_duration {
            error!(
                ?now,
                genesis_era_end=?genesis_timestamp + era_duration,
                "node started with no trusted hash after the expected end of the genesis era - \
                 specify a trusted hash and restart");
            return fatal!(effect_builder, "should have trusted hash after genesis era").ignore();
        }

        info!("initial run at genesis");
        effect_builder
            .commit_genesis(Arc::clone(&self.chainspec))
            .event(Event::CommitGenesisResult)
    }

    fn commit_upgrade(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        upgrade_block_header: BlockHeader,
    ) -> Effects<Event> {
        info!("committing upgrade");
        let global_state_update = match self.chainspec.protocol_config.get_update_mapping() {
            Ok(state_update) => state_update,
            Err(error) => {
                return fatal!(
                    effect_builder,
                    "failed to get global state update from config: {}",
                    error
                )
                .ignore();
            }
        };
        let upgrade_config = UpgradeConfig::new(
            *upgrade_block_header.state_root_hash(),
            upgrade_block_header.protocol_version(),
            self.chainspec.protocol_version(),
            Some(self.chainspec.protocol_config.activation_point.era_id()),
            Some(self.chainspec.core_config.validator_slots),
            Some(self.chainspec.core_config.auction_delay),
            Some(self.chainspec.core_config.locked_funds_period.millis()),
            Some(self.chainspec.core_config.round_seigniorage_rate),
            Some(self.chainspec.core_config.unbonding_delay),
            global_state_update,
        );
        effect_builder
            .upgrade_contract_runtime(Box::new(upgrade_config))
            .event(|result| Event::UpgradeResult {
                upgrade_block_header,
                result,
            })
    }

    fn handle_commit_genesis_result(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        result: Result<GenesisSuccess, engine_state::Error>,
    ) -> Effects<Event> {
        match result {
            Ok(GenesisSuccess {
                post_state_hash, ..
            }) => {
                info!(
                    "genesis chainspec name {}",
                    self.chainspec.network_config.name
                );
                info!("genesis state root hash {}", post_state_hash);

                let genesis_timestamp = match self.genesis_timestamp() {
                    None => {
                        return fatal!(effect_builder, "must have genesis timestamp").ignore();
                    }
                    Some(timestamp) => timestamp,
                };

                let next_block_height = 0;
                let initial_pre_state = ExecutionPreState::new(
                    next_block_height,
                    post_state_hash,
                    Default::default(),
                    Default::default(),
                );
                let finalized_block = FinalizedBlock::new(
                    BlockPayload::default(),
                    Some(EraReport::default()),
                    genesis_timestamp,
                    EraId::default(),
                    next_block_height,
                    PublicKey::System,
                );

                self.execute_block(effect_builder, initial_pre_state, finalized_block)
            }
            Err(error) => {
                error!(%error, "failed to commit genesis");
                fatal!(effect_builder, "{}", error).ignore()
            }
        }
    }

    fn handle_upgrade_result(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        upgrade_block_header: BlockHeader,
        result: Result<UpgradeSuccess, engine_state::Error>,
    ) -> Effects<Event> {
        match result {
            Ok(UpgradeSuccess {
                post_state_hash, ..
            }) => {
                info!(
                    network_name = %self.chainspec.network_config.name,
                    %post_state_hash,
                    "upgrade committed"
                );

                let initial_pre_state = ExecutionPreState::new(
                    upgrade_block_header.height() + 1,
                    post_state_hash,
                    upgrade_block_header.hash(
                        self.chainspec
                            .protocol_config
                            .verifiable_chunked_hash_activation,
                    ),
                    upgrade_block_header.accumulated_seed(),
                );
                let finalized_block = FinalizedBlock::new(
                    BlockPayload::default(),
                    Some(EraReport::default()),
                    upgrade_block_header.timestamp(),
                    upgrade_block_header.next_block_era_id(),
                    initial_pre_state.next_block_height(),
                    PublicKey::System,
                );
                self.execute_block(effect_builder, initial_pre_state, finalized_block)
            }
            Err(error) => {
                error!(%error, "failed to commit upgrade");
                fatal!(effect_builder, "{}", error).ignore()
            }
        }
    }

    /// Creates a switch block after an upgrade or genesis. This block has the system public key as
    /// a proposer and doesn't contain any deploys or transfers. It is the only block in its era,
    /// and no consensus instance is run for era 0 or an upgrade point era.
    fn execute_block(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        initial_pre_state: ExecutionPreState,
        finalized_block: FinalizedBlock,
    ) -> Effects<Event> {
        let protocol_version = self.chainspec.protocol_version();
        async move {
            let block_and_execution_effects = effect_builder
                .execute_finalized_block(
                    protocol_version,
                    initial_pre_state,
                    finalized_block,
                    vec![],
                    vec![],
                )
                .await?;
            // We need to store the block now so that the era supervisor can be properly
            // initialized in the participating reactor's constructor.
            effect_builder
                .put_block_to_storage(block_and_execution_effects.block.clone())
                .await;
            Ok(block_and_execution_effects)
        }
        .event(Event::ExecuteBlockResult)
    }

    fn handle_execute_block_result(
        &mut self,
        effect_builder: EffectBuilder<JoinerEvent>,
        result: Result<BlockAndExecutionEffects, BlockExecutionError>,
    ) -> Effects<Event> {
        match result {
            Ok(block_and_execution_effects) => {
                self.joining_outcome = Some(JoiningOutcome::RanUpgradeOrGenesis {
                    block_and_execution_effects,
                });
                Effects::new()
            }
            Err(error) => {
                error!(%error, "failed to execute block");
                fatal!(effect_builder, "{}", error).ignore()
            }
        }
    }

    fn handle_got_next_upgrade(&mut self, next_upgrade: ActivationPoint) -> Effects<Event> {
        self.maybe_next_upgrade = Some(next_upgrade);
        Effects::new()
    }

    fn genesis_timestamp(&self) -> Option<Timestamp> {
        self.chainspec
            .protocol_config
            .activation_point
            .genesis_timestamp()
    }
}

impl Component<JoinerEvent> for ChainSynchronizer {
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<JoinerEvent>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::HighestBlockHash(maybe_highest_block) => {
                self.handle_highest_block(effect_builder, maybe_highest_block)
            }
            Event::SyncResult(result) => self.handle_sync_result(effect_builder, result),
            Event::CommitGenesisResult(result) => {
                self.handle_commit_genesis_result(effect_builder, result)
            }
            Event::UpgradeResult {
                upgrade_block_header,
                result,
            } => self.handle_upgrade_result(effect_builder, upgrade_block_header, result),
            Event::ExecuteBlockResult(result) => {
                self.handle_execute_block_result(effect_builder, result)
            }
            Event::GotUpgradeActivationPoint(next_upgrade) => {
                self.handle_got_next_upgrade(next_upgrade)
            }
        }
    }
}
