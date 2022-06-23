mod config;
mod error;
mod event;
mod metrics;
mod operations;

use std::{collections::HashSet, convert::Infallible, fmt::Debug, marker::PhantomData, sync::Arc};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, info};

use casper_execution_engine::{
    core::engine_state::{self, genesis::GenesisSuccess, UpgradeSuccess},
    storage::trie::TrieOrChunk,
};
use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::{
        consensus::EraReport,
        contract_runtime::{BlockAndExecutionEffects, BlockExecutionError, ExecutionPreState},
        Component,
    },
    effect::{
        announcements::{BlocklistAnnouncement, ControlAnnouncement},
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, FetcherRequest, NetworkInfoRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    reactor::{joiner::JoinerEvent, participating::ParticipatingEvent},
    storage::StorageRequest,
    types::{
        ActivationPoint, Block, BlockAndDeploys, BlockHeader, BlockHeaderWithMetadata,
        BlockHeadersBatch, BlockPayload, BlockWithMetadata, Chainspec, Deploy,
        FinalizedApprovalsWithId, FinalizedBlock, NodeConfig,
    },
    NodeRng, SmallNetworkConfig,
};
use config::Config;
pub(crate) use error::Error;
pub(crate) use event::Event;
pub(crate) use metrics::Metrics;
use operations::FastSyncOutcome;
pub(crate) use operations::KeyBlockInfo;

#[derive(DataSize, Debug)]
pub(crate) enum JoiningOutcome {
    /// We need to shutdown for upgrade as we downloaded a block from a higher protocol version.
    ShouldExitForUpgrade,
    /// We finished initial synchronizing, with the given block header being the result of the fast
    /// sync task.
    Synced { highest_block_header: BlockHeader },
    /// We ran `commit_genesis` or `commit_upgrade` and created the given switch block immediately
    /// afterwards. `highest_block_header` will be the same as that in
    /// `block_and_execution_effects` except where we synced using a trusted block of the last
    /// switch block before an emergency upgrade, in which case it might be a later block.
    RanUpgradeOrGenesis {
        block_and_execution_effects: BlockAndExecutionEffects,
        validators_to_sign_immediate_switch_block: HashSet<PublicKey>,
        highest_block_header: BlockHeader,
    },
}

#[derive(DataSize, Debug)]
pub(crate) struct ChainSynchronizer<REv> {
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
    /// Association with the reactor event used in subtasks.
    _phantom: PhantomData<REv>,
}

impl ChainSynchronizer<JoinerEvent> {
    /// Constructs a new `ChainSynchronizer` suitable for use in the joiner reactor to perform the
    /// initial fast sync.
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        node_config: NodeConfig,
        small_network_config: SmallNetworkConfig,
        maybe_next_upgrade: Option<ActivationPoint>,
        effect_builder: EffectBuilder<JoinerEvent>,
        registry: &Registry,
    ) -> Result<(Self, Effects<Event>), Error> {
        let synchronizer = ChainSynchronizer {
            config: Config::new(chainspec, node_config, small_network_config),
            joining_outcome: None,
            metrics: Metrics::new(registry)?,
            maybe_next_upgrade,
            _phantom: PhantomData,
        };
        let effects = synchronizer.fast_sync(effect_builder);
        Ok((synchronizer, effects))
    }

    pub(crate) fn metrics(&self) -> Metrics {
        self.metrics.clone()
    }
}

impl ChainSynchronizer<ParticipatingEvent> {
    /// Constructs a new `ChainSynchronizer` suitable for use in the participating reactor to sync
    /// to genesis.
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        node_config: NodeConfig,
        small_network_config: SmallNetworkConfig,
        maybe_next_upgrade: Option<ActivationPoint>,
        metrics: Metrics,
        effect_builder: EffectBuilder<ParticipatingEvent>,
    ) -> Result<(Self, Effects<Event>), Error> {
        let synchronizer = ChainSynchronizer {
            config: Config::new(chainspec, node_config, small_network_config),
            joining_outcome: None,
            metrics,
            maybe_next_upgrade,
            _phantom: PhantomData,
        };

        // If we're not configured to sync-to-genesis, return without doing anything.
        if !synchronizer.config.sync_to_genesis() {
            return Ok((synchronizer, Effects::new()));
        }

        let effects = operations::run_sync_to_genesis_task(
            effect_builder,
            synchronizer.config.clone(),
            synchronizer.metrics.clone(),
        )
        .ignore();

        Ok((synchronizer, effects))
    }
}

impl<REv> ChainSynchronizer<REv>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<BlockHeadersBatch>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockWithMetadata>>
        + From<FetcherRequest<BlockHeaderWithMetadata>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalizedApprovalsWithId>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<BlocklistAnnouncement>
        + From<ControlAnnouncement>
        + Send,
{
    pub(crate) fn joining_outcome(&self) -> Option<&JoiningOutcome> {
        self.joining_outcome.as_ref()
    }

    pub(crate) fn into_joining_outcome(self) -> Option<JoiningOutcome> {
        self.joining_outcome
    }

    fn fast_sync(&self, effect_builder: EffectBuilder<REv>) -> Effects<Event> {
        operations::run_fast_sync_task(effect_builder, self.config.clone(), self.metrics.clone())
            .event(Event::FastSyncResult)
    }

    fn handle_fast_sync_result(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        result: Result<FastSyncOutcome, Error>,
    ) -> Effects<Event> {
        match result {
            Ok(FastSyncOutcome::ShouldCommitGenesis) => self.commit_genesis(effect_builder),
            Ok(FastSyncOutcome::ShouldCommitUpgrade {
                switch_block_header_before_upgrade,
                is_emergency_upgrade,
            }) => self.commit_upgrade(
                effect_builder,
                switch_block_header_before_upgrade,
                is_emergency_upgrade,
            ),
            Ok(FastSyncOutcome::Synced {
                highest_block_header,
            }) => {
                self.joining_outcome = Some(JoiningOutcome::Synced {
                    highest_block_header,
                });
                Effects::new()
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

    fn commit_genesis(&self, effect_builder: EffectBuilder<REv>) -> Effects<Event> {
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
        effect_builder: EffectBuilder<REv>,
        switch_block_header_before_upgrade: BlockHeader,
        is_emergency_upgrade: bool,
    ) -> Effects<Event> {
        info!(%is_emergency_upgrade, "committing upgrade");
        let config = self.config.clone();
        let cloned_block_header = switch_block_header_before_upgrade.clone();
        async move {
            let chainspec_raw_bytes = effect_builder.get_chainspec_raw_bytes().await;
            let upgrade_config =
                match config.new_upgrade_config(&cloned_block_header, chainspec_raw_bytes) {
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
        .event(move |result| Event::UpgradeResult {
            switch_block_header_before_upgrade,
            is_emergency_upgrade,
            result,
        })
    }

    fn handle_commit_genesis_result(
        &self,
        effect_builder: EffectBuilder<REv>,
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
                    false,
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
        effect_builder: EffectBuilder<REv>,
        switch_block_header_before_upgrade: BlockHeader,
        is_emergency_upgrade: bool,
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
                    switch_block_header_before_upgrade.height() + 1,
                    post_state_hash,
                    switch_block_header_before_upgrade
                        .hash(self.config.verifiable_chunked_hash_activation()),
                    switch_block_header_before_upgrade.accumulated_seed(),
                );
                let finalized_block = FinalizedBlock::new(
                    BlockPayload::default(),
                    Some(EraReport::default()),
                    switch_block_header_before_upgrade.timestamp(),
                    switch_block_header_before_upgrade.next_block_era_id(),
                    initial_pre_state.next_block_height(),
                    PublicKey::System,
                );
                // If this is an emergency upgrade, we don't need to pass the switch block from just
                // before the upgrade, as it's only used to derive the list of validators to sign
                // the immediate switch block, and for an emergency upgrade that list is the same as
                // its own `next_era_validators` collection.
                let maybe_switch_block_header_before_upgrade =
                    (!is_emergency_upgrade).then(|| switch_block_header_before_upgrade);

                self.execute_immediate_switch_block(
                    effect_builder,
                    maybe_switch_block_header_before_upgrade,
                    initial_pre_state,
                    finalized_block,
                    is_emergency_upgrade,
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
        effect_builder: EffectBuilder<REv>,
        maybe_switch_block_header_before_upgrade: Option<BlockHeader>,
        initial_pre_state: ExecutionPreState,
        finalized_block: FinalizedBlock,
        is_emergency_upgrade: bool,
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
        .event(move |result| Event::ExecuteImmediateSwitchBlockResult {
            maybe_switch_block_header_before_upgrade,
            is_emergency_upgrade,
            result,
        })
    }

    fn handle_execute_immediate_switch_block_result(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        maybe_switch_block_header_before_upgrade: Option<BlockHeader>,
        is_emergency_upgrade: bool,
        result: Result<BlockAndExecutionEffects, BlockExecutionError>,
    ) -> Effects<Event> {
        let immediate_switch_block_and_exec_effects = match result {
            Ok(block_and_execution_effects) => block_and_execution_effects,
            Err(error) => {
                error!(%error, "failed to execute block");
                return fatal!(effect_builder, "{}", error).ignore();
            }
        };

        // If the switch block before the immediate switch block is `None`, we use the
        // `next_era_validators` of the immediate switch block to sign it.  This is the case at
        // genesis and for an emergency upgrade.
        let maybe_era_end = maybe_switch_block_header_before_upgrade
            .as_ref()
            .unwrap_or_else(|| immediate_switch_block_and_exec_effects.block.header())
            .era_end();

        let validators_to_sign_immediate_switch_block = match maybe_era_end {
            Some(era_end) => era_end
                .next_era_validator_weights()
                .keys()
                .cloned()
                .collect(),
            None => {
                error!("upgrade/genesis switch block missing era end");
                return fatal!(
                    effect_builder,
                    "upgrade/genesis switch block missing era end"
                )
                .ignore();
            }
        };

        // For an emergency upgrade, we always execute/commit locally rather than syncing over it.
        // This means we should try fast syncing again if this was an emergency upgrade.
        if is_emergency_upgrade {
            return operations::run_fast_sync_task(
                effect_builder,
                self.config.clone(),
                self.metrics.clone(),
            )
            .event(|result| Event::FastSyncAfterEmergencyUpgradeResult {
                immediate_switch_block_and_exec_effects,
                validators_to_sign_immediate_switch_block,
                result,
            });
        }

        let highest_block_header = immediate_switch_block_and_exec_effects
            .block
            .header()
            .clone();
        self.joining_outcome = Some(JoiningOutcome::RanUpgradeOrGenesis {
            block_and_execution_effects: immediate_switch_block_and_exec_effects,
            validators_to_sign_immediate_switch_block,
            highest_block_header,
        });
        Effects::new()
    }

    fn handle_fast_sync_after_emergency_upgrade_result(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        immediate_switch_block_and_exec_effects: BlockAndExecutionEffects,
        validators_to_sign_immediate_switch_block: HashSet<PublicKey>,
        result: Result<FastSyncOutcome, Error>,
    ) -> Effects<Event> {
        match result {
            Ok(FastSyncOutcome::ShouldCommitGenesis) => {
                let msg = "fast sync after emergency upgrade should not require commit genesis";
                error!(msg);
                fatal!(effect_builder, "{}", msg).ignore()
            }
            Ok(FastSyncOutcome::ShouldCommitUpgrade { .. }) => {
                let msg = "fast sync after emergency upgrade should not require commit upgrade";
                error!(msg);
                fatal!(effect_builder, "{}", msg).ignore()
            }
            Ok(FastSyncOutcome::Synced {
                highest_block_header,
            }) => {
                self.joining_outcome = Some(JoiningOutcome::RanUpgradeOrGenesis {
                    block_and_execution_effects: immediate_switch_block_and_exec_effects,
                    validators_to_sign_immediate_switch_block,
                    highest_block_header,
                });
                Effects::new()
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

    fn handle_got_next_upgrade(&mut self, next_upgrade: ActivationPoint) -> Effects<Event> {
        self.maybe_next_upgrade = Some(next_upgrade);
        Effects::new()
    }
}

impl<REv> Component<REv> for ChainSynchronizer<REv>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<BlockHeadersBatch>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockWithMetadata>>
        + From<FetcherRequest<BlockHeaderWithMetadata>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalizedApprovalsWithId>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<BlocklistAnnouncement>
        + From<ControlAnnouncement>
        + Send,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::FastSyncResult(result) => self.handle_fast_sync_result(effect_builder, result),
            Event::CommitGenesisResult(result) => {
                self.handle_commit_genesis_result(effect_builder, result)
            }
            Event::UpgradeResult {
                switch_block_header_before_upgrade,
                is_emergency_upgrade,
                result,
            } => self.handle_upgrade_result(
                effect_builder,
                switch_block_header_before_upgrade,
                is_emergency_upgrade,
                result,
            ),
            Event::ExecuteImmediateSwitchBlockResult {
                maybe_switch_block_header_before_upgrade,
                is_emergency_upgrade,
                result,
            } => self.handle_execute_immediate_switch_block_result(
                effect_builder,
                maybe_switch_block_header_before_upgrade,
                is_emergency_upgrade,
                result,
            ),
            Event::FastSyncAfterEmergencyUpgradeResult {
                immediate_switch_block_and_exec_effects,
                validators_to_sign_immediate_switch_block,
                result,
            } => self.handle_fast_sync_after_emergency_upgrade_result(
                effect_builder,
                immediate_switch_block_and_exec_effects,
                validators_to_sign_immediate_switch_block,
                result,
            ),
            Event::GotUpgradeActivationPoint(next_upgrade) => {
                self.handle_got_next_upgrade(next_upgrade)
            }
        }
    }
}
