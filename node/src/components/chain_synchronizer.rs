mod config;
mod error;
mod event;
mod metrics;
mod operations;

use std::{collections::HashSet, convert::Infallible, fmt::Debug, sync::Arc};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, info};

use casper_execution_engine::core::engine_state::{self, genesis::GenesisSuccess, UpgradeSuccess};
use casper_types::{EraId, PublicKey, Timestamp};

use self::metrics::Metrics;
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
        NodeConfig,
    },
    NodeRng, SmallNetworkConfig,
};
use config::Config;
pub(crate) use error::Error;
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
        validators_to_sign_immediate_switch_block: HashSet<PublicKey>,
    },
}

#[derive(DataSize, Debug)]
pub(crate) struct ChainSynchronizer {
    config: Config,
    /// This will be populated once the synchronizer has completed all work, indicating the joiner
    /// reactor can stop running.  It is passed to the participating reactor's constructor via its
    /// config.
    joining_outcome: Option<JoiningOutcome>,
    /// Metrics for the chain synchronization process.
    metrics: Metrics,
    /// The next upgrade activation point, used to determine what action to take after completing
    /// chain synchronization.
    maybe_next_upgrade: Option<ActivationPoint>,
}

impl ChainSynchronizer {
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        node_config: NodeConfig,
        small_network_config: SmallNetworkConfig,
        maybe_next_upgrade: Option<ActivationPoint>,
        verifiable_chunked_hash_activation: EraId,
        effect_builder: EffectBuilder<JoinerEvent>,
        registry: &Registry,
    ) -> Result<(Self, Effects<Event>), Error> {
        let synchronizer = ChainSynchronizer {
            config: Config::new(chainspec, node_config, small_network_config),
            joining_outcome: None,
            metrics: Metrics::new(registry)?,
            maybe_next_upgrade,
        };
        let effects = match synchronizer.config.trusted_hash() {
            None => {
                // If no trusted hash was provided in the config, get the highest block from storage
                // in order to use its hash, or in the case of no blocks, to commit genesis.
                effect_builder
                    .get_highest_block_header_from_storage()
                    .event(move |maybe_highest_block_header| {
                        Event::HighestBlockHash(maybe_highest_block_header.map(|block_header| {
                            block_header.hash(verifiable_chunked_hash_activation)
                        }))
                    })
            }
            Some(trusted_hash) => synchronizer.start_syncing(effect_builder, trusted_hash),
        };
        Ok((synchronizer, effects))
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
            self.config.clone(),
            self.metrics.clone(),
            trusted_hash,
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
                if latest_block_header.protocol_version() == self.config.protocol_version() {
                    self.joining_outcome = Some(JoiningOutcome::Synced {
                        latest_block_header,
                    });
                    Effects::new()
                } else if self
                    .config
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
        let genesis_timestamp = match self.config.genesis_timestamp() {
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
        let era_duration = self.config.era_duration();
        if now > genesis_timestamp + era_duration {
            error!(
                ?now,
                genesis_era_end=?genesis_timestamp + era_duration,
                "node started with no trusted hash after the expected end of the genesis era - \
                 specify a trusted hash and restart");
            return fatal!(effect_builder, "should have trusted hash after genesis era").ignore();
        }

        info!("initial run at genesis");
        let chainspec = self.config.chainspec();
        async move {
            let chainspec_raw_bytes = effect_builder.get_chainspec_raw_bytes().await;
            effect_builder
                .commit_genesis(chainspec, chainspec_raw_bytes)
                .await
        }
        .event(Event::CommitGenesisResult)
    }

    fn commit_upgrade(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        upgrade_block_header: BlockHeader,
    ) -> Effects<Event> {
        info!("committing upgrade");
        let config = self.config.clone();
        let cloned_upgrade_block_header = upgrade_block_header.clone();
        async move {
            let chainspec_raw_bytes = effect_builder.get_chainspec_raw_bytes().await;
            let upgrade_config = match config
                .new_upgrade_config(&cloned_upgrade_block_header, chainspec_raw_bytes)
            {
                Ok(state_update) => state_update,
                Err(error) => {
                    error!(?error, "failed to get global state update from config");
                    return Err(error.into());
                }
            };
            effect_builder
                .upgrade_contract_runtime(upgrade_config)
                .await
        }
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
                info!("genesis chainspec name {}", self.config.network_name());
                info!("genesis state root hash {}", post_state_hash);

                let genesis_timestamp = match self.config.genesis_timestamp() {
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

                self.execute_immediate_switch_block(
                    effect_builder,
                    None,
                    initial_pre_state,
                    finalized_block,
                )
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
                    network_name = %self.config.network_name(),
                    %post_state_hash,
                    "upgrade committed"
                );

                let initial_pre_state = ExecutionPreState::new(
                    upgrade_block_header.height() + 1,
                    post_state_hash,
                    upgrade_block_header.hash(self.config.verifiable_chunked_hash_activation()),
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
                self.execute_immediate_switch_block(
                    effect_builder,
                    Some(upgrade_block_header),
                    initial_pre_state,
                    finalized_block,
                )
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
    fn execute_immediate_switch_block(
        &self,
        effect_builder: EffectBuilder<JoinerEvent>,
        maybe_upgrade_block_header: Option<BlockHeader>,
        initial_pre_state: ExecutionPreState,
        finalized_block: FinalizedBlock,
    ) -> Effects<Event> {
        let protocol_version = self.config.protocol_version();
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
        .event(|result| Event::ExecuteImmediateSwitchBlockResult {
            maybe_upgrade_block_header,
            result,
        })
    }

    fn handle_execute_immediate_switch_block_result(
        &mut self,
        effect_builder: EffectBuilder<JoinerEvent>,
        maybe_upgrade_block_header: Option<BlockHeader>,
        result: Result<BlockAndExecutionEffects, BlockExecutionError>,
    ) -> Effects<Event> {
        let block_and_execution_effects = match result {
            Ok(block_and_execution_effects) => block_and_execution_effects,
            Err(error) => {
                error!(%error, "failed to execute block");
                return fatal!(effect_builder, "{}", error).ignore();
            }
        };

        // If the upgrade block is `None`, this is a genesis switch block, so we can use the
        // validator set from that.
        let maybe_era_end = maybe_upgrade_block_header
            .as_ref()
            .unwrap_or_else(|| block_and_execution_effects.block.header())
            .era_end();
        let validators_to_sign_immediate_switch_block = match maybe_era_end {
            Some(era_end) => era_end
                .next_era_validator_weights()
                .keys()
                .cloned()
                .collect(),
            None => {
                error!(
                    ?maybe_upgrade_block_header,
                    "upgrade/genesis block is not a switch block"
                );
                return fatal!(
                    effect_builder,
                    "upgrade/genesis block is not a switch block"
                )
                .ignore();
            }
        };
        self.joining_outcome = Some(JoiningOutcome::RanUpgradeOrGenesis {
            block_and_execution_effects,
            validators_to_sign_immediate_switch_block,
        });
        Effects::new()
    }

    fn handle_got_next_upgrade(&mut self, next_upgrade: ActivationPoint) -> Effects<Event> {
        self.maybe_next_upgrade = Some(next_upgrade);
        Effects::new()
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
            Event::ExecuteImmediateSwitchBlockResult {
                maybe_upgrade_block_header,
                result,
            } => self.handle_execute_immediate_switch_block_result(
                effect_builder,
                maybe_upgrade_block_header,
                result,
            ),
            Event::GotUpgradeActivationPoint(next_upgrade) => {
                self.handle_got_next_upgrade(next_upgrade)
            }
        }
    }
}
