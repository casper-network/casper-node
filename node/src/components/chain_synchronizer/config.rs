use std::{sync::Arc, time::Duration};

use datasize::DataSize;
use num::rational::Ratio;

use casper_types::{EraId, ProtocolVersion, TimeDiff};

use crate::{
    components::consensus::ChainspecConsensusExt,
    types::{BlockHash, BlockHeader, Chainspec, NodeConfig},
    SmallNetworkConfig,
};

#[derive(Clone, DataSize, Debug)]
pub(super) struct Config {
    chainspec: Arc<Chainspec>,
    /// Hash used as a trust anchor when joining, if any.
    trusted_hash: Option<BlockHash>,
    /// Maximum number of deploys to fetch in parallel.
    max_parallel_deploy_fetches: u32,
    /// Maximum number of trie nodes to fetch in parallel.
    max_parallel_trie_fetches: u32,
    /// Maximum number of blocks to fetch in parallel.
    max_parallel_block_fetches: u32,
    /// The duration for which to pause between retry attempts while synchronising.
    retry_interval: Duration,
    /// Whether to run in sync-to-genesis mode which captures all data (blocks, deploys
    /// and global state) back to genesis.
    sync_to_genesis: bool,
    /// The maximum number of consecutive times we'll allow the network component to return an
    /// empty set of fully-connected peers before we give up.
    max_retries_while_not_connected: u64,
    /// How many fetches in between attempting to redeem one bad node.
    pub(crate) redemption_interval: u32,
    /// Block and block header fetch operations are retried forever until we have enough connected
    /// peers. If the operation fails while there are enough peers, the process gives up. By
    /// default, this is the number of items on the `known_addresses` list in the node config
    /// reduced by one.
    pub(crate) minimum_peer_count_threshold_for_block_fetch_retry: usize,
}

impl Config {
    pub(super) fn new(
        chainspec: Arc<Chainspec>,
        node_config: NodeConfig,
        small_network_config: SmallNetworkConfig,
    ) -> Self {
        let total_retry_ms = 3 * small_network_config.gossip_interval.millis() + 10_000;
        let max_retries_while_not_connected = total_retry_ms / node_config.retry_interval.millis();

        Config {
            chainspec: Arc::clone(&chainspec),
            trusted_hash: node_config.trusted_hash,
            max_parallel_deploy_fetches: node_config.max_parallel_deploy_fetches,
            max_parallel_trie_fetches: node_config.max_parallel_trie_fetches,
            max_parallel_block_fetches: node_config.max_parallel_block_fetches,
            retry_interval: Duration::from_millis(node_config.retry_interval.millis()),
            sync_to_genesis: node_config.sync_to_genesis,
            max_retries_while_not_connected,
            redemption_interval: node_config.sync_peer_redemption_interval,
            minimum_peer_count_threshold_for_block_fetch_retry: small_network_config
                .known_addresses
                .len()
                .saturating_sub(1),
        }
    }

    pub(super) fn protocol_version(&self) -> ProtocolVersion {
        self.chainspec.protocol_config.version
    }

    pub(super) fn era_duration(&self) -> TimeDiff {
        self.chainspec.core_config.era_duration
    }

    pub(super) fn min_era_height(&self) -> u64 {
        self.chainspec.core_config.minimum_era_height
    }

    pub(super) fn auction_delay(&self) -> u64 {
        self.chainspec.core_config.auction_delay
    }

    pub(super) fn unbonding_delay(&self) -> u64 {
        self.chainspec.core_config.unbonding_delay
    }

    pub(super) fn finality_threshold_fraction(&self) -> Ratio<u64> {
        self.chainspec.highway_config.finality_threshold_fraction
    }

    pub(super) fn deploy_max_ttl(&self) -> TimeDiff {
        self.chainspec.deploy_config.max_ttl
    }

    pub(super) fn min_round_length(&self) -> TimeDiff {
        self.chainspec.highway_config.min_round_length()
    }

    pub(super) fn trusted_hash(&self) -> Option<BlockHash> {
        self.trusted_hash
    }

    pub(super) fn max_parallel_deploy_fetches(&self) -> usize {
        self.max_parallel_deploy_fetches as usize
    }

    pub(super) fn max_parallel_trie_fetches(&self) -> usize {
        self.max_parallel_trie_fetches as usize
    }

    pub(super) fn max_parallel_block_fetches(&self) -> usize {
        self.max_parallel_block_fetches as usize
    }

    pub(super) fn retry_interval(&self) -> Duration {
        self.retry_interval
    }

    pub(super) fn sync_to_genesis(&self) -> bool {
        self.sync_to_genesis
    }

    pub(super) fn max_retries_while_not_connected(&self) -> u64 {
        self.max_retries_while_not_connected
    }

    /// Returns `ChainspecConsensusExt::earliest_open_era`.
    pub(super) fn earliest_open_era(&self, current_era: EraId) -> EraId {
        self.chainspec.earliest_open_era(current_era)
    }

    /// Returns `ChainspecConsensusExt::earliest_switch_block_needed`.
    pub(super) fn earliest_switch_block_needed(&self, era_id: EraId) -> EraId {
        self.chainspec.earliest_switch_block_needed(era_id)
    }

    /// Returns `ProtocolConfig::is_last_block_before_activation`.
    pub(super) fn is_last_block_before_activation(&self, block_header: &BlockHeader) -> bool {
        self.chainspec
            .protocol_config
            .is_last_block_before_activation(block_header)
    }

    pub(super) fn chainspec(&self) -> Arc<Chainspec> {
        Arc::clone(&self.chainspec)
    }
}
