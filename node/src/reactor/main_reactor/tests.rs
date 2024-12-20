mod binary_port;
mod transactions;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::TryFrom,
    iter,
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use casper_binary_port::{
    BinaryMessage, BinaryMessageCodec, BinaryRequest, BinaryRequestHeader,
    BinaryResponseAndRequest, InformationRequest, Uptime,
};
use either::Either;
use futures::{SinkExt, StreamExt};
use num::Zero;
use num_rational::Ratio;
use num_traits::One;
use rand::Rng;
use tempfile::TempDir;
use tokio::{
    net::TcpStream,
    time::{self, error::Elapsed, timeout},
};
use tokio_util::codec::Framed;
use tracing::{error, info};

use casper_storage::{
    data_access_layer::{
        balance::{BalanceHandling, BalanceResult},
        BalanceRequest, BidsRequest, BidsResult, ProofHandling, TotalSupplyRequest,
        TotalSupplyResult,
    },
    global_state::state::{StateProvider, StateReader},
};
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    execution::{ExecutionResult, ExecutionResultV2, TransformKindV2, TransformV2},
    system::{
        auction::{BidAddr, BidKind, BidsExt, DelegationRate, DelegatorKind},
        AUCTION,
    },
    testing::TestRng,
    AccountConfig, AccountsConfig, ActivationPoint, AddressableEntityHash, AvailableBlockRange,
    Block, BlockHash, BlockHeader, BlockV2, CLValue, Chainspec, ChainspecRawBytes,
    ConsensusProtocolName, Deploy, EraId, FeeHandling, Gas, HoldBalanceHandling, Key, Motes,
    NextUpgrade, Peers, PricingHandling, PricingMode, ProtocolVersion, PublicKey, RefundHandling,
    Rewards, SecretKey, StoredValue, SystemHashRegistry, TimeDiff, Timestamp, Transaction,
    TransactionHash, TransactionV1Config, ValidatorConfig, U512,
};

use crate::{
    components::{
        consensus::{
            self, ClContext, ConsensusMessage, HighwayMessage, HighwayVertex, NewBlockPayload,
        },
        gossiper, network, storage,
    },
    effect::{
        incoming::ConsensusMessageIncoming,
        requests::{ContractRuntimeRequest, NetworkRequest},
        EffectExt,
    },
    failpoints::FailpointActivation,
    protocol::Message,
    reactor::{
        main_reactor::{Config, MainEvent, MainReactor, ReactorState},
        Reactor, Runner,
    },
    testing::{
        self, filter_reactor::FilterReactor, network::TestingNetwork, ConditionCheckReactor,
    },
    types::{
        transaction::transaction_v1_builder::TransactionV1Builder, BlockPayload, ExitCode, NodeId,
        SyncHandling,
    },
    utils::{External, Loadable, Source, RESOURCES_PATH},
    WithDir,
};

const ERA_ZERO: EraId = EraId::new(0);
const ERA_ONE: EraId = EraId::new(1);
const ERA_TWO: EraId = EraId::new(2);
const ERA_THREE: EraId = EraId::new(3);
const TEN_SECS: Duration = Duration::from_secs(10);
const THIRTY_SECS: Duration = Duration::from_secs(30);
const ONE_MIN: Duration = Duration::from_secs(60);

type Nodes = testing::network::Nodes<FilterReactor<MainReactor>>;

impl Runner<ConditionCheckReactor<FilterReactor<MainReactor>>> {
    fn main_reactor(&self) -> &MainReactor {
        self.reactor().inner().inner()
    }
}

enum InitialStakes {
    FromVec(Vec<u128>),
    Random { count: usize },
    AllEqual { count: usize, stake: u128 },
}

/// Options to allow overriding default chainspec and config settings.
struct ConfigsOverride {
    era_duration: TimeDiff,
    minimum_block_time: TimeDiff,
    minimum_era_height: u64,
    unbonding_delay: u64,
    round_seigniorage_rate: Ratio<u64>,
    consensus_protocol: ConsensusProtocolName,
    finders_fee: Ratio<u64>,
    finality_signature_proportion: Ratio<u64>,
    signature_rewards_max_delay: u64,
    storage_multiplier: u8,
    max_gas_price: u8,
    min_gas_price: u8,
    upper_threshold: u64,
    lower_threshold: u64,
    max_block_size: u32,
    block_gas_limit: u64,
    refund_handling_override: Option<RefundHandling>,
    fee_handling_override: Option<FeeHandling>,
    pricing_handling_override: Option<PricingHandling>,
    allow_prepaid_override: Option<bool>,
    balance_hold_interval_override: Option<TimeDiff>,
    administrators: Option<BTreeSet<PublicKey>>,
    chain_name: Option<String>,
    gas_hold_balance_handling: Option<HoldBalanceHandling>,
    transaction_v1_override: Option<TransactionV1Config>,
    node_config_override: NodeConfigOverride,
}

impl ConfigsOverride {
    fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.refund_handling_override = Some(refund_handling);
        self
    }

    fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling_override = Some(fee_handling);
        self
    }

    fn with_pricing_handling(mut self, pricing_handling: PricingHandling) -> Self {
        self.pricing_handling_override = Some(pricing_handling);
        self
    }

    #[allow(unused)]
    fn with_allow_prepaid(mut self, allow_prepaid: bool) -> Self {
        self.allow_prepaid_override = Some(allow_prepaid);
        self
    }

    fn with_balance_hold_interval(mut self, balance_hold_interval: TimeDiff) -> Self {
        self.balance_hold_interval_override = Some(balance_hold_interval);
        self
    }

    fn with_min_gas_price(mut self, min_gas_price: u8) -> Self {
        self.min_gas_price = min_gas_price;
        self
    }

    fn with_max_gas_price(mut self, max_gas_price: u8) -> Self {
        self.max_gas_price = max_gas_price;
        self
    }

    fn with_lower_threshold(mut self, lower_threshold: u64) -> Self {
        self.lower_threshold = lower_threshold;
        self
    }

    fn with_upper_threshold(mut self, upper_threshold: u64) -> Self {
        self.upper_threshold = upper_threshold;
        self
    }

    fn with_block_size(mut self, max_block_size: u32) -> Self {
        self.max_block_size = max_block_size;
        self
    }

    fn with_block_gas_limit(mut self, block_gas_limit: u64) -> Self {
        self.block_gas_limit = block_gas_limit;
        self
    }

    fn with_minimum_era_height(mut self, minimum_era_height: u64) -> Self {
        self.minimum_era_height = minimum_era_height;
        self
    }

    fn with_administrators(mut self, administrators: BTreeSet<PublicKey>) -> Self {
        self.administrators = Some(administrators);
        self
    }

    fn with_chain_name(mut self, chain_name: String) -> Self {
        self.chain_name = Some(chain_name);
        self
    }

    fn with_gas_hold_balance_handling(
        mut self,
        gas_hold_balance_handling: HoldBalanceHandling,
    ) -> Self {
        self.gas_hold_balance_handling = Some(gas_hold_balance_handling);
        self
    }

    fn with_transaction_v1_config(mut self, transaction_v1config: TransactionV1Config) -> Self {
        self.transaction_v1_override = Some(transaction_v1config);
        self
    }
}

#[derive(Clone, Default)]
struct NodeConfigOverride {
    sync_handling_override: Option<SyncHandling>,
}

impl Default for ConfigsOverride {
    fn default() -> Self {
        ConfigsOverride {
            era_duration: TimeDiff::from_millis(0), // zero means use the default value
            minimum_block_time: "1second".parse().unwrap(),
            minimum_era_height: 2,
            unbonding_delay: 3,
            round_seigniorage_rate: Ratio::new(1, 100),
            consensus_protocol: ConsensusProtocolName::Zug,
            finders_fee: Ratio::new(1, 4),
            finality_signature_proportion: Ratio::new(1, 3),
            signature_rewards_max_delay: 5,
            storage_multiplier: 1,
            max_gas_price: 3,
            min_gas_price: 1,
            upper_threshold: 90,
            lower_threshold: 50,
            max_block_size: 10_485_760u32,
            block_gas_limit: 10_000_000_000_000u64,
            refund_handling_override: None,
            fee_handling_override: None,
            pricing_handling_override: None,
            allow_prepaid_override: None,
            balance_hold_interval_override: None,
            administrators: None,
            chain_name: None,
            gas_hold_balance_handling: None,
            transaction_v1_override: None,
            node_config_override: NodeConfigOverride::default(),
        }
    }
}

struct NodeContext {
    id: NodeId,
    secret_key: Arc<SecretKey>,
    config: Config,
    storage_dir: TempDir,
}

struct TestFixture {
    rng: TestRng,
    node_contexts: Vec<NodeContext>,
    network: TestingNetwork<FilterReactor<MainReactor>>,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,
}

impl TestFixture {
    /// Sets up a new fixture with the number of nodes indicated by `initial_stakes`.
    ///
    /// Runs the network until all nodes are initialized (i.e. none of their reactor states are
    /// still `ReactorState::Initialize`).
    async fn new(initial_stakes: InitialStakes, spec_override: Option<ConfigsOverride>) -> Self {
        let mut rng = TestRng::new();
        let stake_values = match initial_stakes {
            InitialStakes::FromVec(stakes) => {
                stakes.into_iter().map(|stake| stake.into()).collect()
            }
            InitialStakes::Random { count } => {
                // By default we use very large stakes so we would catch overflow issues.
                iter::from_fn(|| Some(U512::from(rng.gen_range(100..999)) * U512::from(u128::MAX)))
                    .take(count)
                    .collect()
            }
            InitialStakes::AllEqual { count, stake } => {
                vec![stake.into(); count]
            }
        };

        let secret_keys: Vec<Arc<SecretKey>> = (0..stake_values.len())
            .map(|_| Arc::new(SecretKey::random(&mut rng)))
            .collect();

        let stakes = secret_keys
            .iter()
            .zip(stake_values)
            .map(|(secret_key, stake)| (PublicKey::from(secret_key.as_ref()), stake))
            .collect();
        Self::new_with_keys(rng, secret_keys, stakes, spec_override).await
    }

    async fn new_with_keys(
        mut rng: TestRng,
        secret_keys: Vec<Arc<SecretKey>>,
        stakes: BTreeMap<PublicKey, U512>,
        spec_override: Option<ConfigsOverride>,
    ) -> Self {
        testing::init_logging();

        // Load the `local` chainspec.
        let (mut chainspec, chainspec_raw_bytes) =
            <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        let min_motes = 100_000_000_000_000_000u64;
        let max_motes = min_motes * 100;
        let balance = U512::from(rng.gen_range(min_motes..max_motes));

        // Override accounts with those generated from the keys.
        let accounts = stakes
            .into_iter()
            .map(|(public_key, bonded_amount)| {
                let validator_config =
                    ValidatorConfig::new(Motes::new(bonded_amount), DelegationRate::zero());
                AccountConfig::new(public_key, Motes::new(balance), Some(validator_config))
            })
            .collect();
        let delegators = vec![];
        let administrators = vec![];
        chainspec.network_config.accounts_config =
            AccountsConfig::new(accounts, delegators, administrators);

        // Allow 2 seconds startup time per validator.
        let genesis_time = Timestamp::now() + TimeDiff::from_seconds(secret_keys.len() as u32 * 2);
        info!(
            "creating test chain configuration, genesis: {}",
            genesis_time
        );
        chainspec.protocol_config.activation_point = ActivationPoint::Genesis(genesis_time);
        chainspec.core_config.finality_threshold_fraction = Ratio::new(34, 100);
        chainspec.core_config.era_duration = TimeDiff::from_millis(0);
        chainspec.core_config.auction_delay = 1;
        chainspec.core_config.validator_slots = 100;
        let ConfigsOverride {
            era_duration,
            minimum_block_time,
            minimum_era_height,
            unbonding_delay,
            round_seigniorage_rate,
            consensus_protocol,
            finders_fee,
            finality_signature_proportion,
            signature_rewards_max_delay,
            storage_multiplier,
            max_gas_price,
            min_gas_price,
            upper_threshold,
            lower_threshold,
            max_block_size,
            block_gas_limit,
            refund_handling_override,
            fee_handling_override,
            pricing_handling_override,
            allow_prepaid_override,
            balance_hold_interval_override,
            administrators,
            chain_name,
            gas_hold_balance_handling,
            transaction_v1_override,
            node_config_override,
        } = spec_override.unwrap_or_default();
        if era_duration != TimeDiff::from_millis(0) {
            chainspec.core_config.era_duration = era_duration;
        }
        info!(?block_gas_limit);
        chainspec.core_config.minimum_block_time = minimum_block_time;
        chainspec.core_config.minimum_era_height = minimum_era_height;
        chainspec.core_config.unbonding_delay = unbonding_delay;
        chainspec.core_config.round_seigniorage_rate = round_seigniorage_rate;
        chainspec.core_config.consensus_protocol = consensus_protocol;
        chainspec.core_config.finders_fee = finders_fee;
        chainspec.core_config.finality_signature_proportion = finality_signature_proportion;
        chainspec.core_config.minimum_block_time = minimum_block_time;
        chainspec.core_config.minimum_era_height = minimum_era_height;
        chainspec.vacancy_config.min_gas_price = min_gas_price;
        chainspec.vacancy_config.max_gas_price = max_gas_price;
        chainspec.vacancy_config.upper_threshold = upper_threshold;
        chainspec.vacancy_config.lower_threshold = lower_threshold;
        chainspec.transaction_config.block_gas_limit = block_gas_limit;
        chainspec.transaction_config.max_block_size = max_block_size;
        chainspec.highway_config.maximum_round_length =
            chainspec.core_config.minimum_block_time * 2;
        chainspec.core_config.signature_rewards_max_delay = signature_rewards_max_delay;

        if let Some(refund_handling) = refund_handling_override {
            chainspec.core_config.refund_handling = refund_handling;
        }
        if let Some(fee_handling) = fee_handling_override {
            chainspec.core_config.fee_handling = fee_handling;
        }
        if let Some(pricing_handling) = pricing_handling_override {
            chainspec.core_config.pricing_handling = pricing_handling;
        }
        if let Some(allow_prepaid) = allow_prepaid_override {
            chainspec.core_config.allow_prepaid = allow_prepaid;
        }
        if let Some(balance_hold_interval) = balance_hold_interval_override {
            chainspec.core_config.gas_hold_interval = balance_hold_interval;
        }
        if let Some(administrators) = administrators {
            chainspec.core_config.administrators = administrators;
        }
        if let Some(chain_name) = chain_name {
            chainspec.network_config.name = chain_name;
        }
        if let Some(gas_hold_balance_handling) = gas_hold_balance_handling {
            chainspec.core_config.gas_hold_balance_handling = gas_hold_balance_handling;
        }
        if let Some(transaction_v1_config) = transaction_v1_override {
            chainspec.transaction_config.transaction_v1_config = transaction_v1_config
        }

        let applied_block_gas_limit = chainspec.transaction_config.block_gas_limit;

        info!(?applied_block_gas_limit);

        let mut fixture = TestFixture {
            rng,
            node_contexts: vec![],
            network: TestingNetwork::new(),
            chainspec: Arc::new(chainspec),
            chainspec_raw_bytes: Arc::new(chainspec_raw_bytes),
        };

        for secret_key in secret_keys {
            let (config, storage_dir) = fixture.create_node_config(
                secret_key.as_ref(),
                None,
                storage_multiplier,
                node_config_override.clone(),
            );
            fixture.add_node(secret_key, config, storage_dir).await;
        }

        fixture
            .run_until(
                move |nodes: &Nodes| {
                    nodes.values().all(|runner| {
                        !matches!(runner.main_reactor().state, ReactorState::Initialize)
                    })
                },
                Duration::from_secs(20),
            )
            .await;

        fixture
    }

    /// Access the environments RNG.
    #[inline(always)]
    pub fn rng_mut(&mut self) -> &mut TestRng {
        &mut self.rng
    }

    /// Returns the highest complete block from node 0.
    ///
    /// Panics if there is no such block.
    #[track_caller]
    fn highest_complete_block(&self) -> Block {
        let node_0 = self
            .node_contexts
            .first()
            .expect("should have at least one node")
            .id;
        self.network
            .nodes()
            .get(&node_0)
            .expect("should have node 0")
            .main_reactor()
            .storage()
            .get_highest_complete_block()
            .expect("should not error reading db")
            .expect("node 0 should have a complete block")
    }

    /// Get block by height
    fn get_block_by_height(&self, block_height: u64) -> Block {
        let node_0 = self
            .node_contexts
            .first()
            .expect("should have at least one node")
            .id;

        self.network
            .nodes()
            .get(&node_0)
            .expect("should have node 0")
            .main_reactor()
            .storage()
            .read_block_by_height(block_height)
            .expect("failure to read block at height")
    }

    #[track_caller]
    fn get_block_gas_price_by_public_key(&self, maybe_public_key: Option<&PublicKey>) -> u8 {
        let node_id = match maybe_public_key {
            None => {
                &self
                    .node_contexts
                    .first()
                    .expect("should have at least one node")
                    .id
            }
            Some(public_key) => {
                let (node_id, _) = self
                    .network
                    .nodes()
                    .iter()
                    .find(|(_, runner)| runner.main_reactor().consensus.public_key() == public_key)
                    .expect("should have runner");

                node_id
            }
        };

        self.network
            .nodes()
            .get(node_id)
            .expect("should have node 0")
            .main_reactor()
            .storage()
            .get_highest_complete_block()
            .expect("should not error reading db")
            .expect("node 0 should have a complete block")
            .maybe_current_gas_price()
            .expect("must have gas price")
    }

    #[track_caller]
    fn switch_block(&self, era: EraId) -> BlockV2 {
        let node_0 = self
            .node_contexts
            .first()
            .expect("should have at least one node")
            .id;
        self.network
            .nodes()
            .get(&node_0)
            .expect("should have node 0")
            .main_reactor()
            .storage()
            .read_switch_block_by_era_id(era)
            .and_then(|block| BlockV2::try_from(block).ok())
            .unwrap_or_else(|| panic!("node 0 should have a switch block V2 for {}", era))
    }

    #[track_caller]
    fn create_node_config(
        &mut self,
        secret_key: &SecretKey,
        maybe_trusted_hash: Option<BlockHash>,
        storage_multiplier: u8,
        node_config_override: NodeConfigOverride,
    ) -> (Config, TempDir) {
        // Set the network configuration.
        let network_cfg = match self.node_contexts.first() {
            Some(first_node) => {
                let known_address =
                    SocketAddr::from_str(&first_node.config.network.bind_address).unwrap();
                network::Config::default_local_net(known_address.port())
            }
            None => {
                let port = testing::unused_port_on_localhost();
                network::Config::default_local_net_first_node(port)
            }
        };
        let mut cfg = Config {
            network: network_cfg,
            gossip: gossiper::Config::new_with_small_timeouts(),
            binary_port_server: crate::BinaryPortConfig {
                allow_request_get_all_values: true,
                allow_request_get_trie: true,
                allow_request_speculative_exec: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let NodeConfigOverride {
            sync_handling_override,
        } = node_config_override;
        if let Some(sync_handling) = sync_handling_override {
            cfg.node.sync_handling = sync_handling;
        }

        // Additionally set up storage in a temporary directory.
        let (storage_cfg, temp_dir) = storage::Config::new_for_tests(storage_multiplier);
        // ...and the secret key for our validator.
        {
            let secret_key_path = temp_dir.path().join("secret_key");
            secret_key
                .to_file(secret_key_path.clone())
                .expect("could not write secret key");
            cfg.consensus.secret_key_path = External::Path(secret_key_path);
        }
        cfg.storage = storage_cfg;
        cfg.node.trusted_hash = maybe_trusted_hash;
        cfg.contract_runtime.max_global_state_size =
            Some(1024 * 1024 * storage_multiplier as usize);

        (cfg, temp_dir)
    }

    /// Adds a node to the network.
    ///
    /// If a previously-removed node is to be re-added, then the `secret_key`, `config` and
    /// `storage_dir` returned in the `NodeContext` during removal should be used here in order to
    /// ensure the same storage dir is used across both executions.
    async fn add_node(
        &mut self,
        secret_key: Arc<SecretKey>,
        config: Config,
        storage_dir: TempDir,
    ) -> NodeId {
        let (id, _) = self
            .network
            .add_node_with_config_and_chainspec(
                WithDir::new(RESOURCES_PATH.join("local"), config.clone()),
                Arc::clone(&self.chainspec),
                Arc::clone(&self.chainspec_raw_bytes),
                &mut self.rng,
            )
            .await
            .expect("could not add node to reactor");
        let node_context = NodeContext {
            id,
            secret_key,
            config,
            storage_dir,
        };
        self.node_contexts.push(node_context);
        info!("added node {} with id {}", self.node_contexts.len() - 1, id);
        id
    }

    #[track_caller]
    fn remove_and_stop_node(&mut self, index: usize) -> NodeContext {
        let node_context = self.node_contexts.remove(index);
        let runner = self.network.remove_node(&node_context.id).unwrap();
        runner.is_shutting_down.set();
        info!("removed node {} with id {}", index, node_context.id);
        node_context
    }

    /// Runs the network until `condition` is true.
    ///
    /// Returns an error if the condition isn't met in time.
    async fn try_run_until<F>(&mut self, condition: F, within: Duration) -> Result<(), Elapsed>
    where
        F: Fn(&Nodes) -> bool,
    {
        self.network
            .try_settle_on(&mut self.rng, condition, within)
            .await
    }

    /// Runs the network until `condition` is true.
    ///
    /// Panics if the condition isn't met in time.
    async fn run_until<F>(&mut self, condition: F, within: Duration)
    where
        F: Fn(&Nodes) -> bool,
    {
        self.network
            .settle_on(&mut self.rng, condition, within)
            .await
    }

    /// Runs the network until all nodes reach the given completed block height.
    ///
    /// Returns an error if the condition isn't met in time.
    async fn try_run_until_block_height(
        &mut self,
        block_height: u64,
        within: Duration,
    ) -> Result<(), Elapsed> {
        self.try_run_until(
            move |nodes: &Nodes| {
                nodes.values().all(|runner| {
                    runner
                        .main_reactor()
                        .storage()
                        .get_highest_complete_block()
                        .expect("should not error reading db")
                        .map(|block| block.height())
                        == Some(block_height)
                })
            },
            within,
        )
        .await
    }

    /// Runs the network until all nodes reach the given completed block height.
    ///
    /// Panics if the condition isn't met in time.
    async fn run_until_block_height(&mut self, block_height: u64, within: Duration) {
        self.try_run_until_block_height(block_height, within)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "should reach block {} within {} seconds",
                    block_height,
                    within.as_secs_f64(),
                )
            })
    }

    /// Runs the network until all nodes' consensus components reach the given era.
    ///
    /// Panics if the condition isn't met in time.
    async fn run_until_consensus_in_era(&mut self, era_id: EraId, within: Duration) {
        self.try_run_until(
            move |nodes: &Nodes| {
                nodes
                    .values()
                    .all(|runner| runner.main_reactor().consensus().current_era() == Some(era_id))
            },
            within,
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "should reach {} within {} seconds",
                era_id,
                within.as_secs_f64(),
            )
        })
    }

    /// Runs the network until all nodes' storage components have stored the switch block header for
    /// the given era.
    ///
    /// Panics if the condition isn't met in time.
    async fn run_until_stored_switch_block_header(&mut self, era_id: EraId, within: Duration) {
        self.try_run_until(
            move |nodes: &Nodes| {
                nodes.values().all(|runner| {
                    let available_block_range =
                        runner.main_reactor().storage().get_available_block_range();
                    runner
                        .main_reactor()
                        .storage()
                        .read_highest_switch_block_headers(1)
                        .unwrap()
                        .last()
                        .map_or(false, |header| {
                            header.era_id() == era_id
                                && available_block_range.contains(header.height())
                        })
                })
            },
            within,
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "should have stored switch block header for {} within {} seconds",
                era_id,
                within.as_secs_f64(),
            )
        })
    }

    /// Runs the network until all nodes have executed the given transaction and stored the
    /// execution result.
    ///
    /// Panics if the condition isn't met in time.
    async fn run_until_executed_transaction(
        &mut self,
        txn_hash: &TransactionHash,
        within: Duration,
    ) {
        self.try_run_until(
            move |nodes: &Nodes| {
                nodes.values().all(|runner| {
                    if runner
                        .main_reactor()
                        .storage()
                        .read_execution_result(txn_hash)
                        .is_some()
                    {
                        let exec_info = runner
                            .main_reactor()
                            .storage()
                            .read_execution_info(*txn_hash);

                        if let Some(exec_info) = exec_info {
                            runner
                                .main_reactor()
                                .storage()
                                .read_block_header_by_height(exec_info.block_height, true)
                                .unwrap()
                                .is_some()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                })
            },
            within,
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "should have stored execution result for {} within {} seconds",
                txn_hash,
                within.as_secs_f64(),
            )
        })
    }

    async fn schedule_upgrade_for_era_two(&mut self) {
        for runner in self.network.runners_mut() {
            runner
                .process_injected_effects(|effect_builder| {
                    let upgrade = NextUpgrade::new(
                        ActivationPoint::EraId(ERA_TWO),
                        ProtocolVersion::from_parts(999, 0, 0),
                    );
                    effect_builder
                        .upgrade_watcher_announcement(Some(upgrade))
                        .ignore()
                })
                .await;
        }
    }

    #[track_caller]
    fn check_bid_existence_at_tip(
        &self,
        validator_public_key: &PublicKey,
        delegator_public_key: Option<&PublicKey>,
        should_exist: bool,
    ) {
        let (_, runner) = self
            .network
            .nodes()
            .iter()
            .find(|(_, runner)| {
                runner.main_reactor().consensus.public_key() == validator_public_key
            })
            .expect("should have runner");

        let highest_block = runner
            .main_reactor()
            .storage
            .read_highest_signed_block(true)
            .expect("should have block")
            .into_inner()
            .0;
        let bids_request = BidsRequest::new(*highest_block.state_root_hash());
        let bids_result = runner
            .main_reactor()
            .contract_runtime
            .data_access_layer()
            .bids(bids_request);

        let delegator_kind = delegator_public_key.map(|pk| DelegatorKind::PublicKey(pk.clone()));

        if let BidsResult::Success { bids } = bids_result {
            match bids.iter().find(|bid_kind| {
                &bid_kind.validator_public_key() == validator_public_key
                    && bid_kind.delegator_kind() == delegator_kind
            }) {
                None => {
                    if should_exist {
                        panic!("should have bid in {}", highest_block.era_id());
                    }
                }
                Some(bid) => {
                    if !should_exist && !bid.is_unbond() {
                        info!("unexpected bid record existence: {:?}", bid);
                        panic!("expected to not have bid");
                    }
                }
            }
        } else {
            panic!("network should have bids: {:?}", bids_result);
        }
    }

    /// Returns the hash of the given system contract.
    #[track_caller]
    fn system_contract_hash(&self, system_contract_name: &str) -> AddressableEntityHash {
        let node_0 = self
            .node_contexts
            .first()
            .expect("should have at least one node")
            .id;
        let reactor = self
            .network
            .nodes()
            .get(&node_0)
            .expect("should have node 0")
            .main_reactor();

        let highest_block = reactor
            .storage
            .read_highest_block()
            .expect("should have block");

        // we need the native auction addr so we can directly call it w/o wasm
        // we can get it out of the system entity registry which is just a
        // value in global state under a stable key.
        let maybe_registry = reactor
            .contract_runtime
            .data_access_layer()
            .checkout(*highest_block.state_root_hash())
            .expect("should checkout")
            .expect("should have view")
            .read(&Key::SystemEntityRegistry)
            .expect("should not have gs storage error")
            .expect("should have stored value");

        let system_entity_registry: SystemHashRegistry = match maybe_registry {
            StoredValue::CLValue(cl_value) => CLValue::into_t(cl_value).unwrap(),
            _ => {
                panic!("expected CLValue")
            }
        };

        (*system_entity_registry.get(system_contract_name).unwrap()).into()
    }

    #[track_caller]
    fn get_current_era_price(&self) -> u8 {
        let (_, runner) = self
            .network
            .nodes()
            .iter()
            .next()
            .expect("must have runner");

        let price = runner.main_reactor().contract_runtime.current_era_price();

        price.gas_price()
    }

    #[track_caller]
    fn check_account_balance_hold_at_tip(&self, account_public_key: PublicKey) -> U512 {
        let (_, runner) = self
            .network
            .nodes()
            .iter()
            .find(|(_, runner)| runner.main_reactor().consensus.public_key() == &account_public_key)
            .expect("must have runner");

        let highest_block = runner
            .main_reactor()
            .storage
            .read_highest_block()
            .expect("should have block");

        let balance_request = BalanceRequest::from_public_key(
            *highest_block.state_root_hash(),
            highest_block.protocol_version(),
            account_public_key,
            BalanceHandling::Available,
            ProofHandling::NoProofs,
        );

        let balance_result = runner
            .main_reactor()
            .contract_runtime
            .data_access_layer()
            .balance(balance_request);

        match balance_result {
            BalanceResult::RootNotFound => {
                panic!("Root not found during balance query")
            }
            BalanceResult::Success { proofs_result, .. } => proofs_result.total_held_amount(),
            BalanceResult::Failure(tce) => {
                panic!("tracking copy error: {:?}", tce)
            }
        }
    }

    async fn inject_transaction(&mut self, txn: Transaction) {
        // saturate the network with the transactions via just making them all store and accept it
        // they're all validators so one of them should propose it
        for runner in self.network.runners_mut() {
            runner
                .process_injected_effects(|effect_builder| {
                    effect_builder
                        .put_transaction_to_storage(txn.clone())
                        .ignore()
                })
                .await;
            runner
                .process_injected_effects(|effect_builder| {
                    effect_builder
                        .announce_new_transaction_accepted(Arc::new(txn.clone()), Source::Client)
                        .ignore()
                })
                .await;
        }
    }

    /// Returns the transforms from the stored, successful execution result for the given
    /// transaction from node 0.
    ///
    /// Panics if there is no such execution result, or if it is not a `Success` variant.
    #[track_caller]
    fn successful_execution_transforms(&self, txn_hash: &TransactionHash) -> Vec<TransformV2> {
        let node_0 = self
            .node_contexts
            .first()
            .expect("should have at least one node")
            .id;
        match self
            .network
            .nodes()
            .get(&node_0)
            .expect("should have node 0")
            .main_reactor()
            .storage()
            .read_execution_result(txn_hash)
            .expect("node 0 should have given execution result")
        {
            ExecutionResult::V1(_) => unreachable!(),
            ExecutionResult::V2(ExecutionResultV2 {
                effects,
                consumed: gas,
                error_message,
                ..
            }) => {
                if error_message.is_none() {
                    effects.transforms().to_vec()
                } else {
                    panic!(
                        "transaction execution failed: {:?} gas: {}",
                        error_message, gas
                    );
                }
            }
        }
    }

    #[inline(always)]
    pub fn network_mut(&mut self) -> &mut TestingNetwork<FilterReactor<MainReactor>> {
        &mut self.network
    }

    pub fn run_until_stopped(
        self,
        rng: TestRng,
    ) -> impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)> {
        self.network.crank_until_stopped(rng)
    }
}

/// Given a block height and a node id, returns a predicate to check if the lowest available block
/// for the specified node is at or below the specified height.
fn node_has_lowest_available_block_at_or_below_height(
    height: u64,
    node_id: NodeId,
) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
        nodes.get(&node_id).map_or(true, |runner| {
            let available_block_range = runner.main_reactor().storage().get_available_block_range();
            if available_block_range.low() == 0 && available_block_range.high() == 0 {
                false
            } else {
                available_block_range.low() <= height
            }
        })
    }
}

fn is_ping(event: &MainEvent) -> bool {
    if let MainEvent::ConsensusMessageIncoming(ConsensusMessageIncoming { message, .. }) = event {
        if let ConsensusMessage::Protocol { ref payload, .. } = **message {
            return matches!(
                payload.deserialize_incoming::<HighwayMessage<ClContext>>(),
                Ok(HighwayMessage::<ClContext>::NewVertex(HighwayVertex::Ping(
                    _
                )))
            );
        }
    }
    false
}

/// A set of consecutive switch blocks.
struct SwitchBlocks {
    headers: Vec<BlockHeader>,
}

impl SwitchBlocks {
    /// Collects all switch blocks of the first `era_count` eras, and asserts that they are equal
    /// in all nodes.
    fn collect(nodes: &Nodes, era_count: u64) -> SwitchBlocks {
        let mut headers = Vec::new();
        for era_number in 0..era_count {
            let mut header_iter = nodes.values().map(|runner| {
                let storage = runner.main_reactor().storage();
                let maybe_block = storage.read_switch_block_by_era_id(EraId::from(era_number));
                maybe_block.expect("missing switch block").take_header()
            });
            let header = header_iter.next().unwrap();
            assert_eq!(era_number, header.era_id().value());
            for other_header in header_iter {
                assert_eq!(header, other_header);
            }
            headers.push(header);
        }
        SwitchBlocks { headers }
    }

    /// Returns the list of equivocators in the given era.
    fn equivocators(&self, era_number: u64) -> &[PublicKey] {
        self.headers[era_number as usize]
            .maybe_equivocators()
            .expect("era end")
    }

    /// Returns the list of inactive validators in the given era.
    fn inactive_validators(&self, era_number: u64) -> &[PublicKey] {
        self.headers[era_number as usize]
            .maybe_inactive_validators()
            .expect("era end")
    }

    /// Returns the list of validators in the successor era.
    fn next_era_validators(&self, era_number: u64) -> &BTreeMap<PublicKey, U512> {
        self.headers[era_number as usize]
            .next_era_validator_weights()
            .expect("validators")
    }

    /// Returns the set of bids in the auction contract at the end of the given era.
    fn bids(&self, nodes: &Nodes, era_number: u64) -> Vec<BidKind> {
        let state_root_hash = *self.headers[era_number as usize].state_root_hash();
        for runner in nodes.values() {
            let request = BidsRequest::new(state_root_hash);
            let data_provider = runner.main_reactor().contract_runtime().data_access_layer();
            if let BidsResult::Success { bids } = data_provider.bids(request) {
                return bids;
            }
        }
        unreachable!("at least one node should have bids for era {}", era_number);
    }
}

#[tokio::test]
async fn run_network() {
    // Set up a network with five nodes and run until in era 2.
    let initial_stakes = InitialStakes::Random { count: 5 };
    let mut fixture = TestFixture::new(initial_stakes, None).await;
    fixture.run_until_consensus_in_era(ERA_TWO, ONE_MIN).await;
}

#[tokio::test]
async fn historical_sync_with_era_height_1() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_block_time: "4seconds".parse().unwrap(),
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Wait for all nodes to reach era 3.
    fixture.run_until_consensus_in_era(ERA_THREE, ONE_MIN).await;

    // Create a joiner node.
    let secret_key = SecretKey::random(&mut fixture.rng);
    let trusted_hash = *fixture.highest_complete_block().hash();
    let (mut config, storage_dir) = fixture.create_node_config(
        &secret_key,
        Some(trusted_hash),
        1,
        NodeConfigOverride::default(),
    );
    config.node.sync_handling = SyncHandling::Genesis;
    let joiner_id = fixture
        .add_node(Arc::new(secret_key), config, storage_dir)
        .await;

    // Wait for joiner node to sync back to the block from era 1
    fixture
        .run_until(
            node_has_lowest_available_block_at_or_below_height(1, joiner_id),
            ONE_MIN,
        )
        .await;

    // Remove the weights for era 0 and era 1 from the validator matrix
    let runner = fixture
        .network
        .nodes_mut()
        .get_mut(&joiner_id)
        .expect("Could not find runner for node {joiner_id}");
    let reactor = runner.reactor_mut().inner_mut().inner_mut();
    reactor.validator_matrix.purge_era_validators(&ERA_ZERO);
    reactor.validator_matrix.purge_era_validators(&ERA_ONE);

    // Continue syncing and check if the joiner node reaches era 0
    fixture
        .run_until(
            node_has_lowest_available_block_at_or_below_height(0, joiner_id),
            ONE_MIN,
        )
        .await;
}

#[tokio::test]
async fn should_not_historical_sync_no_sync_node() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_block_time: "4seconds".parse().unwrap(),
        minimum_era_height: 2,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Wait for all nodes to complete block 1.
    fixture.run_until_block_height(1, ONE_MIN).await;

    // Create a joiner node.
    let highest_block = fixture.highest_complete_block();
    let trusted_hash = *highest_block.hash();
    let trusted_height = highest_block.height();
    assert!(
        trusted_height > 0,
        "trusted height must be non-zero to allow for checking that the joiner doesn't do \
        historical syncing"
    );
    info!("joining node using block {trusted_height} {trusted_hash}");
    let secret_key = SecretKey::random(&mut fixture.rng);
    let (mut config, storage_dir) = fixture.create_node_config(
        &secret_key,
        Some(trusted_hash),
        1,
        NodeConfigOverride::default(),
    );
    config.node.sync_handling = SyncHandling::NoSync;
    let joiner_id = fixture
        .add_node(Arc::new(secret_key), config, storage_dir)
        .await;

    let joiner_avail_range = |nodes: &Nodes| {
        nodes
            .get(&joiner_id)
            .expect("should have joiner")
            .main_reactor()
            .storage()
            .get_available_block_range()
    };

    // Run until the joiner doesn't have the default available block range, i.e. it has completed
    // syncing the initial block.
    fixture
        .try_run_until(
            |nodes: &Nodes| joiner_avail_range(nodes) != AvailableBlockRange::RANGE_0_0,
            ONE_MIN,
        )
        .await
        .expect("timed out waiting for joiner to sync first block");

    let available_block_range_pre = joiner_avail_range(fixture.network.nodes());

    let pre = available_block_range_pre.low();
    assert!(
        pre >= trusted_height,
        "should not have acquired a block earlier than trusted hash block {} {}",
        pre,
        trusted_height
    );

    // Ensure the joiner's chain is advancing.
    fixture
        .try_run_until(
            |nodes: &Nodes| joiner_avail_range(nodes).high() > available_block_range_pre.high(),
            ONE_MIN,
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "timed out waiting for joiner's highest complete block to exceed {}",
                available_block_range_pre.high()
            )
        });

    // Ensure the joiner is not doing historical sync.
    fixture
        .try_run_until(
            |nodes: &Nodes| joiner_avail_range(nodes).low() < available_block_range_pre.low(),
            TEN_SECS,
        )
        .await
        .unwrap_err();
}

#[tokio::test]
async fn should_catch_up_and_shutdown() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_block_time: "4seconds".parse().unwrap(),
        minimum_era_height: 2,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Wait for all nodes to complete block 1.
    fixture.run_until_block_height(1, ONE_MIN).await;

    // Create a joiner node.
    let highest_block = fixture.highest_complete_block();
    let trusted_hash = *highest_block.hash();
    let trusted_height = highest_block.height();
    assert!(
        trusted_height > 0,
        "trusted height must be non-zero to allow for checking that the joiner doesn't do \
        historical syncing"
    );

    info!("joining node using block {trusted_height} {trusted_hash}");
    let secret_key = SecretKey::random(&mut fixture.rng);
    let (mut config, storage_dir) = fixture.create_node_config(
        &secret_key,
        Some(trusted_hash),
        1,
        NodeConfigOverride::default(),
    );
    config.node.sync_handling = SyncHandling::CompleteBlock;
    let joiner_id = fixture
        .add_node(Arc::new(secret_key), config, storage_dir)
        .await;

    let joiner_avail_range = |nodes: &Nodes| {
        nodes
            .get(&joiner_id)
            .expect("should have joiner")
            .main_reactor()
            .storage()
            .get_available_block_range()
    };

    // Run until the joiner shuts down after catching up
    fixture
        .network
        .settle_on_node_exit(
            &mut fixture.rng,
            &joiner_id,
            ExitCode::CleanExitDontRestart,
            ONE_MIN,
        )
        .await;

    let available_block_range = joiner_avail_range(fixture.network.nodes());

    let low = available_block_range.low();
    assert!(
        low >= trusted_height,
        "should not have acquired a block earlier than trusted hash block {low} {trusted_hash}",
    );

    let highest_block_height = fixture.highest_complete_block().height();
    let high = available_block_range.high();
    assert!(
        low < high && high <= highest_block_height,
        "should have acquired more recent blocks before shutting down {low} {high} {highest_block_height}",
    );
}

fn network_is_in_keepup(
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>>,
) -> bool {
    nodes
        .values()
        .all(|node| node.reactor().inner().inner().state == ReactorState::KeepUp)
}

const MESSAGE_SIZE: u32 = 1024 * 1024 * 10;

async fn setup_network_and_get_binary_port_handle(
    initial_stakes: InitialStakes,
    spec_override: ConfigsOverride,
) -> (
    Framed<TcpStream, BinaryMessageCodec>,
    impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)>,
) {
    let mut fixture = timeout(
        Duration::from_secs(10),
        TestFixture::new(initial_stakes, Some(spec_override)),
    )
    .await
    .unwrap();
    let mut rng = fixture.rng_mut().create_child();
    let net = fixture.network_mut();
    net.settle_on(&mut rng, network_is_in_keepup, Duration::from_secs(59))
        .await;
    let (_, first_node) = net
        .nodes()
        .iter()
        .next()
        .expect("should have at least one node");
    let binary_port_addr = first_node
        .main_reactor()
        .binary_port
        .bind_address()
        .unwrap();
    let finish_cranking = fixture.run_until_stopped(rng.create_child());
    let address = format!("localhost:{}", binary_port_addr.port());
    let stream = TcpStream::connect(address.clone())
        .await
        .expect("should create stream");
    let client = Framed::new(stream, BinaryMessageCodec::new(MESSAGE_SIZE));
    (client, finish_cranking)
}

#[tokio::test]
async fn should_start_in_isolation() {
    let initial_stakes = InitialStakes::Random { count: 1 };
    let spec_override = ConfigsOverride {
        node_config_override: NodeConfigOverride {
            sync_handling_override: Some(SyncHandling::Isolated),
        },
        ..Default::default()
    };
    let (mut client, finish_cranking) =
        setup_network_and_get_binary_port_handle(initial_stakes, spec_override).await;

    let uptime_request_bytes = {
        let request = BinaryRequest::Get(
            InformationRequest::Uptime
                .try_into()
                .expect("should convert"),
        );
        let header =
            BinaryRequestHeader::new(ProtocolVersion::from_parts(2, 0, 0), request.tag(), 1_u16);
        let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");
        header_bytes
            .iter()
            .chain(
                ToBytes::to_bytes(&request)
                    .expect("should serialize")
                    .iter(),
            )
            .cloned()
            .collect::<Vec<_>>()
    };
    client
        .send(BinaryMessage::new(uptime_request_bytes))
        .await
        .expect("should send message");
    let response = timeout(Duration::from_secs(20), client.next())
        .await
        .unwrap_or_else(|err| panic!("should complete uptime request without timeout: {}", err))
        .unwrap_or_else(|| panic!("should have bytes"))
        .unwrap_or_else(|err| panic!("should have ok response: {}", err));
    let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
        FromBytes::from_bytes(response.payload()).expect("should deserialize response");
    let response = binary_response_and_request.response().payload();
    let (uptime, remainder): (Uptime, _) =
        FromBytes::from_bytes(response).expect("Peers should be deserializable");
    assert!(remainder.is_empty());
    assert!(uptime.into_inner() > 0);
    let (_net, _rng) = timeout(Duration::from_secs(20), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}

#[tokio::test]
async fn should_be_peerless_in_isolation() {
    let initial_stakes = InitialStakes::Random { count: 1 };
    let spec_override = ConfigsOverride {
        node_config_override: NodeConfigOverride {
            sync_handling_override: Some(SyncHandling::Isolated),
        },
        ..Default::default()
    };
    let (mut client, finish_cranking) =
        setup_network_and_get_binary_port_handle(initial_stakes, spec_override).await;

    let peers_request_bytes = {
        let request = BinaryRequest::Get(
            InformationRequest::Peers
                .try_into()
                .expect("should convert"),
        );
        let header =
            BinaryRequestHeader::new(ProtocolVersion::from_parts(2, 0, 0), request.tag(), 1_u16);
        let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");
        header_bytes
            .iter()
            .chain(
                ToBytes::to_bytes(&request)
                    .expect("should serialize")
                    .iter(),
            )
            .cloned()
            .collect::<Vec<_>>()
    };
    client
        .send(BinaryMessage::new(peers_request_bytes))
        .await
        .expect("should send message");
    let response = timeout(Duration::from_secs(20), client.next())
        .await
        .unwrap_or_else(|err| panic!("should complete peers request without timeout: {}", err))
        .unwrap_or_else(|| panic!("should have bytes"))
        .unwrap_or_else(|err| panic!("should have ok response: {}", err));
    let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
        FromBytes::from_bytes(response.payload()).expect("should deserialize response");
    let response = binary_response_and_request.response().payload();

    let (peers, remainder): (Peers, _) =
        FromBytes::from_bytes(response).expect("Peers should be deserializable");
    assert!(remainder.is_empty());
    assert!(
        peers.into_inner().is_empty(),
        "should not have peers in isolated mode"
    );

    let (_net, _rng) = timeout(Duration::from_secs(20), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}

#[tokio::test]
async fn run_equivocator_network() {
    let mut rng = crate::new_rng();

    let alice_secret_key = Arc::new(SecretKey::random(&mut rng));
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::new(SecretKey::random(&mut rng));
    let bob_public_key = PublicKey::from(&*bob_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    let mut stakes = BTreeMap::new();
    stakes.insert(alice_public_key.clone(), U512::from(1));
    stakes.insert(bob_public_key.clone(), U512::from(1));
    stakes.insert(charlie_public_key, U512::from(u64::MAX));

    // Here's where things go wrong: Bob doesn't run a node at all, and Alice runs two!
    let secret_keys = vec![
        alice_secret_key.clone(),
        alice_secret_key,
        charlie_secret_key,
    ];

    // We configure the era to take 15 rounds. That should guarantee that the two nodes equivocate.
    let spec_override = ConfigsOverride {
        minimum_era_height: 10,
        consensus_protocol: ConsensusProtocolName::Highway,
        storage_multiplier: 2,
        ..Default::default()
    };

    let mut fixture =
        TestFixture::new_with_keys(rng, secret_keys, stakes.clone(), Some(spec_override)).await;

    let min_round_len = fixture.chainspec.core_config.minimum_block_time;
    let mut maybe_first_message_time = None;

    let mut alice_reactors = fixture
        .network
        .reactors_mut()
        .filter(|reactor| *reactor.inner().consensus().public_key() == alice_public_key);

    // Delay all messages to and from the first of Alice's nodes until three rounds after the first
    // message.  Further, significantly delay any incoming pings to avoid the node detecting the
    // doppelganger and deactivating itself.
    alice_reactors.next().unwrap().set_filter(move |event| {
        if is_ping(&event) {
            return Either::Left(time::sleep((min_round_len * 30).into()).event(move |_| event));
        }
        let now = Timestamp::now();
        match &event {
            MainEvent::ConsensusMessageIncoming(_) => {}
            MainEvent::NetworkRequest(
                NetworkRequest::SendMessage { payload, .. }
                | NetworkRequest::ValidatorBroadcast { payload, .. }
                | NetworkRequest::Gossip { payload, .. },
            ) if matches!(**payload, Message::Consensus(_)) => {}
            _ => return Either::Right(event),
        };
        let first_message_time = *maybe_first_message_time.get_or_insert(now);
        if now < first_message_time + min_round_len * 3 {
            return Either::Left(time::sleep(min_round_len.into()).event(move |_| event));
        }
        Either::Right(event)
    });

    // Significantly delay all incoming pings to the second of Alice's nodes.
    alice_reactors.next().unwrap().set_filter(move |event| {
        if is_ping(&event) {
            return Either::Left(time::sleep((min_round_len * 30).into()).event(move |_| event));
        }
        Either::Right(event)
    });

    drop(alice_reactors);

    let era_count = 4;

    let timeout = ONE_MIN * (era_count + 1) as u32;
    info!("Waiting for {} eras to end.", era_count);
    fixture
        .run_until_stored_switch_block_header(EraId::new(era_count - 1), timeout)
        .await;

    // network settled; select data to analyze
    let switch_blocks = SwitchBlocks::collect(fixture.network.nodes(), era_count);
    let mut era_bids = BTreeMap::new();
    for era in 0..era_count {
        era_bids.insert(era, switch_blocks.bids(fixture.network.nodes(), era));
    }

    // Since this setup sometimes produces no equivocation or an equivocation in era 2 rather than
    // era 1, we set an offset here.  If neither era has an equivocation, exit early.
    // TODO: Remove this once https://github.com/casper-network/casper-node/issues/1859 is fixed.
    for switch_block in &switch_blocks.headers {
        let era_id = switch_block.era_id();
        let count = switch_blocks.equivocators(era_id.value()).len();
        info!("equivocators in {}: {}", era_id, count);
    }
    let offset = if !switch_blocks.equivocators(1).is_empty() {
        0
    } else if !switch_blocks.equivocators(2).is_empty() {
        error!("failed to equivocate in era 1 - asserting equivocation detected in era 2");
        1
    } else {
        error!("failed to equivocate in era 1 or 2");
        return;
    };

    // Era 0 consists only of the genesis block.
    // In era 1, Alice equivocates. Since eviction takes place with a delay of one
    // (`auction_delay`) era, she is still included in the next era's validator set.
    let next_era_id = 1 + offset;

    assert_eq!(
        switch_blocks.equivocators(next_era_id),
        [alice_public_key.clone()]
    );
    let next_era_bids = era_bids.get(&next_era_id).expect("should have offset era");

    let next_era_alice = next_era_bids
        .validator_bid(&alice_public_key)
        .expect("should have Alice's offset bid");
    assert!(
        next_era_alice.inactive(),
        "Alice's bid should be inactive in offset era."
    );
    assert!(switch_blocks
        .next_era_validators(next_era_id)
        .contains_key(&alice_public_key));

    // In era 2 Alice is banned. Banned validators count neither as faulty nor inactive, even
    // though they cannot participate. In the next era, she will be evicted.
    let future_era_id = 2 + offset;
    assert_eq!(switch_blocks.equivocators(future_era_id), []);
    let future_era_bids = era_bids
        .get(&future_era_id)
        .expect("should have future era");
    let future_era_alice = future_era_bids
        .validator_bid(&alice_public_key)
        .expect("should have Alice's future bid");
    assert!(
        future_era_alice.inactive(),
        "Alice's bid should be inactive in future era."
    );
    assert!(!switch_blocks
        .next_era_validators(future_era_id)
        .contains_key(&alice_public_key));

    // In era 3 Alice is not a validator anymore and her bid remains deactivated.
    let era_3 = 3;
    if offset == 0 {
        assert_eq!(switch_blocks.equivocators(era_3), []);
        let era_3_bids = era_bids.get(&era_3).expect("should have era 3 bids");
        let era_3_alice = era_3_bids
            .validator_bid(&alice_public_key)
            .expect("should have Alice's era 3 bid");
        assert!(
            era_3_alice.inactive(),
            "Alice's bid should be inactive in era 3."
        );
        assert!(!switch_blocks
            .next_era_validators(era_3)
            .contains_key(&alice_public_key));
    }

    // Bob is inactive.
    assert_eq!(
        switch_blocks.inactive_validators(1),
        [bob_public_key.clone()]
    );
    assert_eq!(
        switch_blocks.inactive_validators(2),
        [bob_public_key.clone()]
    );

    for (era, bids) in era_bids {
        for (public_key, stake) in &stakes {
            let bid = bids
                .validator_bid(public_key)
                .expect("should have bid for public key {public_key} in era {era}");
            let staked_amount = bid.staked_amount();
            assert!(
                staked_amount >= *stake,
                "expected stake {} for public key {} in era {}, found {}",
                staked_amount,
                public_key,
                era,
                stake
            );
        }
    }
}

async fn assert_network_shutdown_for_upgrade_with_stakes(initial_stakes: InitialStakes) {
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    // An upgrade is scheduled for era 2, after the switch block in era 1 (height 2).
    fixture.schedule_upgrade_for_era_two().await;

    // Run until the nodes shut down for the upgrade.
    fixture
        .network
        .settle_on_exit(&mut fixture.rng, ExitCode::Success, ONE_MIN)
        .await;
}

#[tokio::test]
async fn nodes_should_have_enough_signatures_before_upgrade_with_equal_stake() {
    // Equal stake ensures that one node was able to learn about signatures created by the other, by
    // whatever means necessary (gossiping, broadcasting, fetching, etc.).
    let initial_stakes = InitialStakes::AllEqual {
        count: 2,
        stake: u128::MAX,
    };
    assert_network_shutdown_for_upgrade_with_stakes(initial_stakes).await;
}

#[tokio::test]
async fn nodes_should_have_enough_signatures_before_upgrade_with_one_dominant_stake() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 255]);
    assert_network_shutdown_for_upgrade_with_stakes(initial_stakes).await;
}

#[tokio::test]
async fn dont_upgrade_without_switch_block() {
    let initial_stakes = InitialStakes::Random { count: 2 };
    let mut fixture = TestFixture::new(initial_stakes, None).await;
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    eprintln!(
        "Running 'dont_upgrade_without_switch_block' test with rng={}",
        fixture.rng
    );

    // An upgrade is scheduled for era 2, after the switch block in era 1 (height 2).
    // We artificially delay the execution of that block.
    fixture.schedule_upgrade_for_era_two().await;
    for runner in fixture.network.runners_mut() {
        let mut exec_request_received = false;
        runner.reactor_mut().inner_mut().set_filter(move |event| {
            if let MainEvent::ContractRuntimeRequest(
                ContractRuntimeRequest::EnqueueBlockForExecution {
                    executable_block, ..
                },
            ) = &event
            {
                if executable_block.era_report.is_some()
                    && executable_block.era_id == ERA_ONE
                    && !exec_request_received
                {
                    info!("delaying {}", executable_block);
                    exec_request_received = true;
                    return Either::Left(
                        time::sleep(Duration::from_secs(10)).event(move |_| event),
                    );
                }
                info!("not delaying {}", executable_block);
            }
            Either::Right(event)
        });
    }

    // Run until the nodes shut down for the upgrade.
    fixture
        .network
        .settle_on_exit(&mut fixture.rng, ExitCode::Success, ONE_MIN)
        .await;

    // Verify that the switch block has been stored: Even though it was delayed the node didn't
    // restart before executing and storing it.
    for runner in fixture.network.nodes().values() {
        let header = runner
            .main_reactor()
            .storage()
            .read_block_header_by_height(2, false)
            .expect("failed to read from storage")
            .expect("missing switch block");
        assert_eq!(ERA_ONE, header.era_id(), "era should be 1");
        assert!(header.is_switch_block(), "header should be switch block");
    }
}

#[tokio::test]
async fn should_store_finalized_approvals() {
    // Set up a network with two nodes where node 0 (Alice) is effectively guaranteed to be the
    // proposer.
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng)); // just for ordering testing purposes

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    // Submit a transaction.
    let mut transaction_alice_bob = Transaction::from(
        Deploy::random_valid_native_transfer_without_deps(&mut fixture.rng),
    );
    let mut transaction_alice_bob_charlie = transaction_alice_bob.clone();
    let mut transaction_bob_alice = transaction_alice_bob.clone();

    transaction_alice_bob.sign(&alice_secret_key);
    transaction_alice_bob.sign(&bob_secret_key);

    transaction_alice_bob_charlie.sign(&alice_secret_key);
    transaction_alice_bob_charlie.sign(&bob_secret_key);
    transaction_alice_bob_charlie.sign(&charlie_secret_key);

    transaction_bob_alice.sign(&bob_secret_key);
    transaction_bob_alice.sign(&alice_secret_key);

    // We will be testing the correct sequence of approvals against the transaction signed by Bob
    // and Alice.
    // The transaction signed by Alice and Bob should give the same ordering of approvals.
    let expected_approvals: Vec<_> = transaction_bob_alice.approvals().iter().cloned().collect();

    // We'll give the transaction signed by Alice, Bob and Charlie to Bob, so these will be his
    // original approvals. Save these for checks later.
    let bobs_original_approvals: Vec<_> = transaction_alice_bob_charlie
        .approvals()
        .iter()
        .cloned()
        .collect();
    assert_ne!(bobs_original_approvals, expected_approvals);

    let transaction_hash = transaction_alice_bob.hash();

    for runner in fixture.network.runners_mut() {
        let transaction = if runner.main_reactor().consensus().public_key() == &alice_public_key {
            // Alice will propose the transaction signed by Alice and Bob.
            transaction_alice_bob.clone()
        } else {
            // Bob will receive the transaction signed by Alice, Bob and Charlie.
            transaction_alice_bob_charlie.clone()
        };
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .put_transaction_to_storage(transaction.clone())
                    .ignore()
            })
            .await;
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .announce_new_transaction_accepted(Arc::new(transaction), Source::Client)
                    .ignore()
            })
            .await;
    }

    // Run until the transaction gets executed.
    let has_stored_exec_results = |nodes: &Nodes| {
        nodes.values().all(|runner| {
            let read = runner
                .main_reactor()
                .storage()
                .read_execution_result(&transaction_hash);
            read.is_some()
        })
    };
    fixture.run_until(has_stored_exec_results, ONE_MIN).await;

    // Check if the approvals agree.
    for runner in fixture.network.nodes().values() {
        let maybe_dwa = runner
            .main_reactor()
            .storage()
            .get_transaction_with_finalized_approvals_by_hash(&transaction_hash);
        let maybe_finalized_approvals = maybe_dwa
            .as_ref()
            .and_then(|dwa| dwa.1.clone())
            .map(|fa| fa.iter().cloned().collect());
        let maybe_original_approvals = maybe_dwa
            .as_ref()
            .map(|(transaction, _approvals)| transaction.approvals().iter().cloned().collect());
        if runner.main_reactor().consensus().public_key() != &alice_public_key {
            // Bob should have finalized approvals, and his original approvals should be different.
            assert_eq!(
                maybe_finalized_approvals.as_ref(),
                Some(&expected_approvals)
            );
            assert_eq!(
                maybe_original_approvals.as_ref(),
                Some(&bobs_original_approvals)
            );
        } else {
            // Alice should only have the correct approvals as the original ones, and no finalized
            // approvals (as they wouldn't be stored, because they would be the same as the
            // original ones).
            assert_eq!(maybe_finalized_approvals.as_ref(), None);
            assert_eq!(maybe_original_approvals.as_ref(), Some(&expected_approvals));
        }
    }
}

// This test exercises a scenario in which a proposed block contains invalid accusations.
// Blocks containing no transactions or transfers used to be incorrectly marked as not needing
// validation even if they contained accusations, which opened up a security hole through which a
// malicious validator could accuse whomever they wanted of equivocating and have these
// accusations accepted by the other validators. This has been patched and the test asserts that
// such a scenario is no longer possible.
#[tokio::test]
async fn empty_block_validation_regression() {
    let initial_stakes = InitialStakes::AllEqual {
        count: 4,
        stake: 100,
    };
    let spec_override = ConfigsOverride {
        minimum_era_height: 15,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    let malicious_validator =
        PublicKey::from(fixture.node_contexts.first().unwrap().secret_key.as_ref());
    info!("Malicious validator: {}", malicious_validator);
    let everyone_else: Vec<_> = fixture
        .node_contexts
        .iter()
        .skip(1)
        .map(|node_context| PublicKey::from(node_context.secret_key.as_ref()))
        .collect();
    let malicious_id = fixture.node_contexts.first().unwrap().id;
    let malicious_runner = fixture.network.nodes_mut().get_mut(&malicious_id).unwrap();
    malicious_runner
        .reactor_mut()
        .inner_mut()
        .set_filter(move |event| match event {
            MainEvent::Consensus(consensus::Event::NewBlockPayload(NewBlockPayload {
                era_id,
                block_payload: _,
                block_context,
            })) => {
                info!("Accusing everyone else!");
                // We hook into the NewBlockPayload event to replace the block being proposed with
                // an empty one that accuses all the validators, except the malicious validator.
                Either::Right(MainEvent::Consensus(consensus::Event::NewBlockPayload(
                    NewBlockPayload {
                        era_id,
                        block_payload: Arc::new(BlockPayload::new(
                            BTreeMap::new(),
                            everyone_else.clone(),
                            Default::default(),
                            false,
                            1u8,
                        )),
                        block_context,
                    },
                )))
            }
            event => Either::Right(event),
        });

    info!("Waiting for the first era after genesis to end.");
    fixture.run_until_consensus_in_era(ERA_TWO, ONE_MIN).await;
    let switch_blocks = SwitchBlocks::collect(fixture.network.nodes(), 2);

    // Nobody actually double-signed. The accusations should have had no effect.
    assert_eq!(
        switch_blocks.equivocators(0),
        [],
        "expected no equivocators"
    );
    // If the malicious validator was the first proposer, all their Highway units might be invalid,
    // because they all refer to the invalid proposal, so they might get flagged as inactive. No
    // other validators should be considered inactive.
    match switch_blocks.inactive_validators(0) {
        [] => {}
        [inactive_validator] if malicious_validator == *inactive_validator => {}
        inactive => panic!("unexpected inactive validators: {:?}", inactive),
    }
}

#[tokio::test]
async fn network_should_recover_from_stall() {
    // Set up a network with three nodes.
    let initial_stakes = InitialStakes::AllEqual {
        count: 3,
        stake: 100,
    };
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    // Let all nodes progress until block 2 is marked complete.
    fixture.run_until_block_height(2, ONE_MIN).await;

    // Kill all nodes except for node 0.
    let mut stopped_nodes = vec![];
    for _ in 1..fixture.node_contexts.len() {
        let node_context = fixture.remove_and_stop_node(1);
        stopped_nodes.push(node_context);
    }

    // Expect node 0 can't produce more blocks, i.e. the network has stalled.
    fixture
        .try_run_until_block_height(3, ONE_MIN)
        .await
        .expect_err("should time out");

    // Restart the stopped nodes.
    for node_context in stopped_nodes {
        fixture
            .add_node(
                node_context.secret_key,
                node_context.config,
                node_context.storage_dir,
            )
            .await;
    }

    // Ensure all nodes progress until block 3 is marked complete.
    fixture.run_until_block_height(3, TEN_SECS).await;
}

#[tokio::test]
async fn run_withdraw_bid_network() {
    let alice_stake = 200_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![alice_stake.into(), 10_000_000_000]);

    let unbonding_delay = 2;

    let mut fixture = TestFixture::new(
        initial_stakes,
        Some(ConfigsOverride {
            unbonding_delay,
            ..Default::default()
        }),
    )
    .await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice has a bid is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);

    // Create & sign deploy to withdraw Alice's full stake.
    let mut deploy = Deploy::withdraw_bid(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        alice_public_key.clone(),
        alice_stake.into(),
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Prune transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::from(alice_public_key.clone()));
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Prune(prune_key) => prune_key == &bid_key,
            _ => false,
        })
        .expect("should have a prune record for bid");

    // Crank the network forward until the era ends.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    // The bid record should have been pruned once unbonding ran.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, false);

    // Crank the network forward until the unbonds are processed.
    fixture
        .run_until_stored_switch_block_header(
            ERA_ONE.saturating_add(unbonding_delay + 1),
            ONE_MIN * 2,
        )
        .await;
}

#[tokio::test]
async fn node_should_rejoin_after_ejection() {
    let initial_stakes = InitialStakes::AllEqual {
        count: 5,
        stake: 1_000_000_000,
    };
    let minimum_era_height = 4;
    let configs_override = ConfigsOverride {
        minimum_era_height,
        minimum_block_time: "4096 ms".parse().unwrap(),
        round_seigniorage_rate: Ratio::new(1, 1_000_000_000_000),
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(configs_override)).await;

    // Run through the first era.
    fixture
        .run_until_block_height(minimum_era_height, ONE_MIN)
        .await;

    let stopped_node = fixture.remove_and_stop_node(1);
    let stopped_secret_key = Arc::clone(&stopped_node.secret_key);
    let stopped_public_key = PublicKey::from(&*stopped_secret_key);

    // Wait until the stopped node is ejected and removed from the validators set.
    fixture
        .run_until_consensus_in_era(
            (fixture.chainspec.core_config.auction_delay + 3).into(),
            ONE_MIN,
        )
        .await;

    // Restart the node.
    // Use the hash of the current highest complete block as the trusted hash.
    let mut config = stopped_node.config;
    config.node.trusted_hash = Some(*fixture.highest_complete_block().hash());
    fixture
        .add_node(stopped_node.secret_key, config, stopped_node.storage_dir)
        .await;

    // Create & sign deploy to reactivate the stopped node's bid.
    // The bid amount will make sure that the rejoining validator proposes soon after it rejoins.
    let mut deploy = Deploy::add_bid(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        stopped_public_key.clone(),
        100_000_000_000_000_000_u64.into(),
        10,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&stopped_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, THIRTY_SECS)
        .await;

    // Ensure execution succeeded and that there is a Write transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::from(stopped_public_key.clone()));
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::BidKind(bid_kind)) => {
                Key::from(bid_kind.bid_addr()) == bid_key
            }
            _ => false,
        })
        .expect("should have a write record for bid");

    // Wait until the auction delay passes, plus one era for a margin of error.
    fixture
        .run_until_consensus_in_era(
            (2 * fixture.chainspec.core_config.auction_delay + 6).into(),
            ONE_MIN,
        )
        .await;
}

#[tokio::test]
async fn run_undelegate_bid_network() {
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![alice_stake.into(), bob_stake.into()]);

    let unbonding_delay = 2;

    let mut fixture = TestFixture::new(
        initial_stakes,
        Some(ConfigsOverride {
            unbonding_delay,
            ..Default::default()
        }),
    )
    .await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_public_key = PublicKey::from(&*fixture.node_contexts[1].secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice and Bob have bids is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    // Alice should not have a delegation bid record for Bob (yet).
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);

    // Have Alice delegate to Bob.
    //
    // Note, in the real world validators usually don't also delegate to other validators,  but in
    // this test fixture the only accounts in the system are those created for genesis validators.
    let alice_delegation_amount =
        U512::from(fixture.chainspec.core_config.minimum_delegation_amount);
    let mut deploy = Deploy::delegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Write transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &bob_public_key,
        Some(&alice_public_key),
    ));
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::BidKind(bid_kind)) => {
                Key::from(bid_kind.bid_addr()) == bid_key
            }
            _ => false,
        })
        .expect("should have a write record for delegate bid");

    // Alice should now have a delegation bid record for Bob.
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), true);

    // Create & sign transaction to undelegate from Alice to Bob.
    let mut deploy = Deploy::undelegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Prune transform for the bid's key.
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Prune(prune_key) => prune_key == &bid_key,
            _ => false,
        })
        .expect("should have a prune record for undelegated bid");

    // Crank the network forward until the era ends.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    // Ensure the validator records are still present but the undelegated bid is gone.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);

    // Crank the network forward until the unbonds are processed.
    fixture
        .run_until_stored_switch_block_header(
            ERA_ONE.saturating_add(unbonding_delay + 1),
            ONE_MIN * 2,
        )
        .await;
}

#[tokio::test]
async fn run_redelegate_bid_network() {
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let charlie_stake = 300_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![
        alice_stake.into(),
        bob_stake.into(),
        charlie_stake.into(),
    ]);

    let spec_override = ConfigsOverride {
        unbonding_delay: 1,
        minimum_era_height: 5,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;
    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_public_key = PublicKey::from(&*fixture.node_contexts[1].secret_key);
    let charlie_public_key = PublicKey::from(&*fixture.node_contexts[2].secret_key);

    // Wait for all nodes to complete block 0.
    fixture.run_until_block_height(0, ONE_MIN).await;

    // Ensure our post genesis assumption that Alice, Bob and Charlie have bids is correct.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    fixture.check_bid_existence_at_tip(&charlie_public_key, None, true);
    // Alice should not have a delegation bid record for Bob or Charlie (yet).
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), false);

    // Have Alice delegate to Bob.
    let alice_delegation_amount =
        U512::from(fixture.chainspec.core_config.minimum_delegation_amount);
    let mut deploy = Deploy::delegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&alice_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, ONE_MIN)
        .await;

    // Ensure execution succeeded and that there is a Write transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &bob_public_key,
        Some(&alice_public_key),
    ));

    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::BidKind(bid_kind)) => {
                Key::from(bid_kind.bid_addr()) == bid_key
            }
            _ => false,
        })
        .expect("should have a write record for delegate bid");

    // Alice should now have a delegation bid record for Bob.
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), true);

    // Create & sign transaction to undelegate Alice from Bob and delegate to Charlie.
    let mut deploy = Deploy::redelegate(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        bob_public_key.clone(),
        alice_public_key.clone(),
        charlie_public_key.clone(),
        alice_delegation_amount,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );

    deploy.sign(&alice_secret_key);
    let transaction = Transaction::Deploy(deploy);
    let transaction_hash = transaction.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(transaction).await;
    fixture
        .run_until_executed_transaction(&transaction_hash, TEN_SECS)
        .await;

    // Ensure execution succeeded and that there is a Prune transform for the bid's key.
    fixture
        .successful_execution_transforms(&transaction_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Prune(prune_key) => prune_key == &bid_key,
            _ => false,
        })
        .expect("should have a prune record for undelegated bid");

    // Original delegation bid should be removed.
    fixture.check_bid_existence_at_tip(&bob_public_key, Some(&alice_public_key), false);
    // Redelegate doesn't occur until after unbonding delay elapses.
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), false);

    // Crank the network forward to run out the unbonding delay.
    // First, close out the era the redelegate was processed in.
    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;
    // The undelegate is in the unbonding queue.
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), false);
    // Unbonding delay is 1 on this test network, so step 1 more era.
    fixture
        .run_until_stored_switch_block_header(ERA_TWO, ONE_MIN)
        .await;

    // Ensure the validator records are still present.
    fixture.check_bid_existence_at_tip(&alice_public_key, None, true);
    fixture.check_bid_existence_at_tip(&bob_public_key, None, true);
    // Ensure redelegated bid exists.
    fixture.check_bid_existence_at_tip(&charlie_public_key, Some(&alice_public_key), true);
}

#[tokio::test]
async fn rewards_are_calculated() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_era_height: 3,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;
    fixture
        .run_until_consensus_in_era(ERA_THREE, Duration::from_secs(150))
        .await;

    let switch_block = fixture.switch_block(ERA_TWO);

    for reward in switch_block
        .era_end()
        .unwrap()
        .rewards()
        .values()
        .map(|amounts| {
            amounts
                .iter()
                .fold(U512::zero(), |acc, amount| *amount + acc)
        })
    {
        assert_ne!(reward, U512::zero());
    }
}

// Reactor pattern tests for simplified rewards

// Fundamental network parameters that are not critical for assessing reward calculation correctness
const STAKE: u128 = 1000000000;
const PRIME_STAKES: [u128; 5] = [106907, 106921, 106937, 106949, 106957];
const ERA_COUNT: u64 = 3;
const ERA_DURATION: u64 = 20000;
//milliseconds
const MIN_HEIGHT: u64 = 6;
const BLOCK_TIME: u64 = 1750;
//milliseconds
const TIME_OUT: u64 = 600;
//seconds
const SEIGNIORAGE: (u64, u64) = (1u64, 100u64);
const REPRESENTATIVE_NODE_INDEX: usize = 0;
// Parameters we generally want to vary
const CONSENSUS_ZUG: ConsensusProtocolName = ConsensusProtocolName::Zug;
const CONSENSUS_HIGHWAY: ConsensusProtocolName = ConsensusProtocolName::Highway;
const FINDERS_FEE_ZERO: (u64, u64) = (0u64, 1u64);
const FINDERS_FEE_HALF: (u64, u64) = (1u64, 2u64);
//const FINDERS_FEE_ONE: (u64, u64) = (1u64, 1u64);
const FINALITY_SIG_PROP_ZERO: (u64, u64) = (0u64, 1u64);
const FINALITY_SIG_PROP_HALF: (u64, u64) = (1u64, 2u64);
const FINALITY_SIG_PROP_ONE: (u64, u64) = (1u64, 1u64);
const FILTERED_NODES_INDICES: &[usize] = &[3, 4];
const FINALITY_SIG_LOOKBACK: u64 = 3;

async fn run_rewards_network_scenario(
    initial_stakes: impl Into<Vec<u128>>,
    era_count: u64,
    time_out: u64, //seconds
    representative_node_index: usize,
    filtered_nodes_indices: &[usize],
    spec_override: ConfigsOverride,
) {
    trait AsU512Ext {
        fn into_u512(self) -> Ratio<U512>;
    }
    impl AsU512Ext for Ratio<u64> {
        fn into_u512(self) -> Ratio<U512> {
            Ratio::new(U512::from(*self.numer()), U512::from(*self.denom()))
        }
    }

    let initial_stakes = initial_stakes.into();

    // Instantiate the chain
    let mut fixture =
        TestFixture::new(InitialStakes::FromVec(initial_stakes), Some(spec_override)).await;

    for i in filtered_nodes_indices {
        let filtered_node = fixture.network.runners_mut().nth(*i).unwrap();
        filtered_node
            .reactor_mut()
            .inner_mut()
            .activate_failpoint(&FailpointActivation::new("finality_signature_creation"));
    }

    // Run the network for a specified number of eras
    let timeout = Duration::from_secs(time_out);
    fixture
        .run_until_stored_switch_block_header(EraId::new(era_count - 1), timeout)
        .await;

    // DATA COLLECTION
    // Get the switch blocks and bid structs first
    let switch_blocks = SwitchBlocks::collect(fixture.network.nodes(), era_count);

    // Representative node
    // (this test should normally run a network at nominal performance with identical nodes)
    let representative_node = fixture
        .network
        .nodes()
        .values()
        .nth(representative_node_index)
        .unwrap();
    let representative_storage = &representative_node.main_reactor().storage;
    let representative_runtime = &representative_node.main_reactor().contract_runtime;

    // Recover highest completed block height
    let highest_completed_height = representative_storage
        .highest_complete_block_height()
        .expect("missing highest completed block");

    // Get all the blocks
    let blocks: Vec<Block> = (0..highest_completed_height + 1)
        .map(|i| {
            representative_storage
                .read_block_by_height(i)
                .expect("block not found")
        })
        .collect();

    let protocol_version = ProtocolVersion::from_parts(2, 0, 0);

    // Get total supply history
    let total_supply: Vec<U512> = (0..highest_completed_height + 1)
        .map(|height: u64| {
            let state_hash = *representative_storage
                .read_block_header_by_height(height, true)
                .expect("failure to read block header")
                .unwrap()
                .state_root_hash();
            let total_supply_req = TotalSupplyRequest::new(state_hash, protocol_version);
            let result = representative_runtime
                .data_access_layer()
                .total_supply(total_supply_req);

            if let TotalSupplyResult::Success { total_supply } = result {
                total_supply
            } else {
                panic!("expected success, not: {:?}", result);
            }
        })
        .collect();

    // Tiny helper function
    #[inline]
    fn add_to_rewards(
        recipient: PublicKey,
        era: EraId,
        reward: Ratio<U512>,
        rewards: &mut BTreeMap<PublicKey, BTreeMap<EraId, Ratio<U512>>>,
    ) {
        match rewards.get_mut(&recipient) {
            Some(map) => {
                *map.entry(era).or_insert(Ratio::zero()) += reward;
            }
            None => {
                let mut map = BTreeMap::new();
                map.insert(era, reward);
                rewards.insert(recipient, map);
            }
        }
    }

    let mut recomputed_total_supply = BTreeMap::new();
    recomputed_total_supply.insert(0, Ratio::from(total_supply[0]));
    let recomputed_rewards: BTreeMap<_, _> = switch_blocks
        .headers
        .iter()
        .enumerate()
        .map(|(i, switch_block)| {
            if switch_block.is_genesis() || switch_block.height() > highest_completed_height {
                return (i, BTreeMap::new());
            }
            let mut recomputed_era_rewards = BTreeMap::new();
            if !switch_block.is_genesis() {
                let supply_carryover = recomputed_total_supply
                    .get(&(i - 1))
                    .copied()
                    .expect("expected prior recomputed supply value");
                recomputed_total_supply.insert(i, supply_carryover);
            }

            // It's not a genesis block, so we know there's something with a lower era id
            let previous_switch_block_height = switch_blocks.headers[i - 1].height();
            let current_era_slated_weights = match switch_blocks.headers[i - 1].clone_era_end() {
                Some(era_report) => era_report.next_era_validator_weights().clone(),
                _ => panic!("unexpectedly absent era report"),
            };
            let total_current_era_weights = current_era_slated_weights
                .iter()
                .fold(U512::zero(), move |acc, s| acc + s.1);
            let weights_block_idx = if switch_blocks.headers[i - 1].is_genesis() {
                i - 1
            } else {
                i - 2
            };
            let (previous_era_slated_weights, total_previous_era_weights) =
                match switch_blocks.headers[weights_block_idx].clone_era_end() {
                    Some(era_report) => {
                        let next_weights = era_report.next_era_validator_weights().clone();
                        let total_next_weights = next_weights
                            .iter()
                            .fold(U512::zero(), move |acc, s| acc + s.1);
                        (next_weights, total_next_weights)
                    }
                    _ => panic!("unexpectedly absent era report"),
                };

            let rewarded_range =
                previous_switch_block_height as usize + 1..switch_block.height() as usize + 1;
            let rewarded_blocks = &blocks[rewarded_range];
            let block_reward = (Ratio::<U512>::one()
                - fixture
                    .chainspec
                    .core_config
                    .finality_signature_proportion
                    .into_u512())
                * recomputed_total_supply[&(i - 1)]
                * fixture
                    .chainspec
                    .core_config
                    .round_seigniorage_rate
                    .into_u512();
            let signatures_reward = fixture
                .chainspec
                .core_config
                .finality_signature_proportion
                .into_u512()
                * recomputed_total_supply[&(i - 1)]
                * fixture
                    .chainspec
                    .core_config
                    .round_seigniorage_rate
                    .into_u512();
            let previous_signatures_reward_idx = if switch_blocks.headers[i - 1].is_genesis() {
                i - 1
            } else {
                i - 2
            };
            let previous_signatures_reward = fixture
                .chainspec
                .core_config
                .finality_signature_proportion
                .into_u512()
                * recomputed_total_supply[&previous_signatures_reward_idx]
                * fixture
                    .chainspec
                    .core_config
                    .round_seigniorage_rate
                    .into_u512();

            rewarded_blocks.iter().for_each(|block: &Block| {
                // Block production rewards
                let proposer = block.proposer().clone();
                add_to_rewards(
                    proposer.clone(),
                    block.era_id(),
                    block_reward,
                    &mut recomputed_era_rewards,
                );

                // Recover relevant finality signatures
                block.rewarded_signatures().iter().enumerate().for_each(
                    |(offset, signatures_packed)| {
                        if block.height() as usize - offset - 1
                            <= previous_switch_block_height as usize
                        {
                            let rewarded_contributors = signatures_packed.to_validator_set(
                                previous_era_slated_weights
                                    .keys()
                                    .cloned()
                                    .collect::<BTreeSet<PublicKey>>(),
                            );
                            rewarded_contributors.iter().for_each(|contributor| {
                                let contributor_proportion = Ratio::new(
                                    previous_era_slated_weights
                                        .get(contributor)
                                        .copied()
                                        .expect("expected current era validator"),
                                    total_previous_era_weights,
                                );
                                // collection always goes to the era in which the block citing the
                                // reward was created
                                add_to_rewards(
                                    proposer.clone(),
                                    block.era_id(),
                                    fixture.chainspec.core_config.finders_fee.into_u512()
                                        * contributor_proportion
                                        * previous_signatures_reward,
                                    &mut recomputed_era_rewards,
                                );
                                add_to_rewards(
                                    contributor.clone(),
                                    switch_blocks.headers[i - 1].era_id(),
                                    (Ratio::<U512>::one()
                                        - fixture.chainspec.core_config.finders_fee.into_u512())
                                        * contributor_proportion
                                        * previous_signatures_reward,
                                    &mut recomputed_era_rewards,
                                )
                            });
                        } else {
                            let rewarded_contributors = signatures_packed.to_validator_set(
                                current_era_slated_weights
                                    .keys()
                                    .cloned()
                                    .collect::<BTreeSet<PublicKey>>(),
                            );
                            rewarded_contributors.iter().for_each(|contributor| {
                                let contributor_proportion = Ratio::new(
                                    *current_era_slated_weights
                                        .get(contributor)
                                        .expect("expected current era validator"),
                                    total_current_era_weights,
                                );
                                add_to_rewards(
                                    proposer.clone(),
                                    block.era_id(),
                                    fixture.chainspec.core_config.finders_fee.into_u512()
                                        * contributor_proportion
                                        * signatures_reward,
                                    &mut recomputed_era_rewards,
                                );
                                add_to_rewards(
                                    contributor.clone(),
                                    block.era_id(),
                                    (Ratio::<U512>::one()
                                        - fixture.chainspec.core_config.finders_fee.into_u512())
                                        * contributor_proportion
                                        * signatures_reward,
                                    &mut recomputed_era_rewards,
                                );
                            });
                        }
                    },
                );
            });

            // Make sure we round just as we do in the real code, at the end of an era's
            // calculation, right before minting and transferring
            recomputed_era_rewards.iter_mut().for_each(|(_, rewards)| {
                rewards.values_mut().for_each(|amount| {
                    *amount = amount.trunc();
                });
                let truncated_reward = rewards.values().sum::<Ratio<U512>>();
                let era_end_supply = recomputed_total_supply
                    .get_mut(&i)
                    .expect("expected supply at end of era");
                *era_end_supply += truncated_reward;
            });

            (i, recomputed_era_rewards)
        })
        .collect();

    // Recalculated total supply is equal to observed total supply
    switch_blocks.headers.iter().for_each(|header| {
        if header.height() <= highest_completed_height {
            assert_eq!(
                Ratio::from(total_supply[header.height() as usize]),
                *(recomputed_total_supply
                    .get(&(header.era_id().value() as usize))
                    .expect("expected recalculated supply")),
                "total supply does not match at height {}",
                header.height()
            );
        }
    });

    // Recalculated rewards are equal to observed rewards; total supply increase is equal to total
    // rewards;
    recomputed_rewards.iter().for_each(|(era, rewards)| {
        if era > &0 && switch_blocks.headers[*era].height() <= highest_completed_height {
            let observed_total_rewards = match switch_blocks.headers[*era]
                .clone_era_end()
                .expect("expected EraEnd")
                .rewards()
            {
                Rewards::V1(v1_rewards) => v1_rewards
                    .iter()
                    .fold(U512::zero(), |acc, reward| U512::from(*reward.1) + acc),
                Rewards::V2(v2_rewards) => v2_rewards
                    .iter()
                    .flat_map(|(_key, amounts)| amounts)
                    .fold(U512::zero(), |acc, reward| *reward + acc),
            };
            let recomputed_total_rewards: U512 = rewards
                .values()
                .flat_map(|amounts| amounts.values().map(|reward| reward.to_integer()))
                .sum();
            assert_eq!(
                Ratio::from(recomputed_total_rewards),
                Ratio::from(observed_total_rewards),
                "total rewards do not match at era {}\nobserved = {:#?}\nrecomputed = {:#?}",
                era,
                switch_blocks.headers[*era]
                    .clone_era_end()
                    .expect("")
                    .rewards(),
                rewards,
            );
            assert_eq!(
                Ratio::from(recomputed_total_rewards),
                recomputed_total_supply
                    .get(era)
                    .expect("expected recalculated supply")
                    - recomputed_total_supply
                        .get(&(era - 1))
                        .expect("expected recalculated supply"),
                "supply growth does not match rewards at era {}",
                era
            )
        }
    })
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_small_prime_five_eras() {
    run_rewards_network_scenario(
        PRIME_STAKES,
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_small_prime_five_eras_no_lookback() {
    run_rewards_network_scenario(
        PRIME_STAKES,
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: 0,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_no_finality_small_nominal_five_eras() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ZERO.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_half_finality_half_finders_small_nominal_five_eras() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_HALF.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_half_finality_half_finders_small_nominal_five_eras_no_lookback() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_HALF.into(),
            signature_rewards_max_delay: 0,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders_small_nominal_five_eras_no_lookback() {
    run_rewards_network_scenario(
        [STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        &[],
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: 0,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders_five_eras() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        5,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_HALF.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_zero_finders() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_ZUG,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_highway_all_finality_zero_finders() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_HIGHWAY,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ONE.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_highway_no_finality() {
    run_rewards_network_scenario(
        [
            STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE,
        ],
        ERA_COUNT,
        TIME_OUT,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES,
        ConfigsOverride {
            consensus_protocol: CONSENSUS_HIGHWAY,
            era_duration: TimeDiff::from_millis(ERA_DURATION),
            minimum_era_height: MIN_HEIGHT,
            minimum_block_time: TimeDiff::from_millis(BLOCK_TIME),
            round_seigniorage_rate: SEIGNIORAGE.into(),
            finders_fee: FINDERS_FEE_ZERO.into(),
            finality_signature_proportion: FINALITY_SIG_PROP_ZERO.into(),
            signature_rewards_max_delay: FINALITY_SIG_LOOKBACK,
            ..Default::default()
        },
    )
    .await;
}

#[allow(clippy::enum_variant_names)]
enum GasPriceScenario {
    SlotUtilization,
    SizeUtilization(u32),
    GasConsumptionUtilization(u64),
}

async fn run_gas_price_scenario(gas_price_scenario: GasPriceScenario) {
    let mut rng = TestRng::new();
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let charlie_stake = 300_000_000_000_u64;
    let initial_stakes: Vec<U512> =
        vec![alice_stake.into(), bob_stake.into(), charlie_stake.into()];

    let mut secret_keys: Vec<Arc<SecretKey>> = (0..3)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();

    let stakes = secret_keys
        .iter()
        .zip(initial_stakes)
        .map(|(secret_key, stake)| (PublicKey::from(secret_key.as_ref()), stake))
        .collect();

    let non_validating_secret_key = SecretKey::random(&mut rng);
    let non_validating_public_key = PublicKey::from(&non_validating_secret_key);
    secret_keys.push(Arc::new(non_validating_secret_key));

    let max_gas_price: u8 = 3;

    let mut transaction_config = TransactionV1Config::default();
    transaction_config.native_mint_lane.max_transaction_count = 1;

    let spec_override = match gas_price_scenario {
        GasPriceScenario::SlotUtilization => {
            ConfigsOverride::default().with_transaction_v1_config(transaction_config)
        }
        GasPriceScenario::SizeUtilization(block_size) => {
            ConfigsOverride::default().with_block_size(block_size)
        }
        GasPriceScenario::GasConsumptionUtilization(gas_limit) => {
            ConfigsOverride::default().with_block_gas_limit(gas_limit)
        }
    }
    .with_lower_threshold(5u64)
    .with_upper_threshold(10u64)
    .with_minimum_era_height(5)
    .with_max_gas_price(max_gas_price);

    let mut fixture =
        TestFixture::new_with_keys(rng, secret_keys, stakes, Some(spec_override)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    let switch_block = fixture.switch_block(ERA_ONE);

    let mut current_era = switch_block.era_id();
    let chain_name = fixture.chainspec.network_config.name.clone();

    // Run the network at load for at least 5 eras.
    for _ in 0..5 {
        let target_public_key = PublicKey::random(&mut fixture.rng);
        let fixed_native_mint_transaction =
            TransactionV1Builder::new_transfer(10_000_000_000u64, None, target_public_key, None)
                .expect("must get builder")
                .with_chain_name(chain_name.clone())
                .with_secret_key(&alice_secret_key)
                .with_ttl(TimeDiff::from_seconds(120 * 10))
                .with_pricing_mode(PricingMode::Fixed {
                    gas_price_tolerance: max_gas_price,
                    additional_computation_factor: 0,
                })
                .build()
                .expect("must get transaction");

        let txn = Transaction::V1(fixed_native_mint_transaction);
        fixture.inject_transaction(txn).await;
        let next_era = current_era.successor();
        fixture
            .run_until_stored_switch_block_header(next_era, ONE_MIN)
            .await;
        current_era = next_era;
    }

    let expected_gas_price = fixture.chainspec.vacancy_config.max_gas_price;
    let actual_gas_price = fixture.get_current_era_price();
    assert_eq!(actual_gas_price, expected_gas_price);
    let gas_price_for_non_validating_node =
        fixture.get_block_gas_price_by_public_key(Some(&non_validating_public_key));
    assert_eq!(actual_gas_price, gas_price_for_non_validating_node);

    let target_public_key = PublicKey::random(&mut fixture.rng);

    let holds_before = fixture.check_account_balance_hold_at_tip(alice_public_key.clone());
    let amount = 10_000_000_000u64;

    let fixed_native_mint_transaction =
        TransactionV1Builder::new_transfer(amount, None, target_public_key, None)
            .expect("must get builder")
            .with_chain_name(chain_name)
            .with_secret_key(&alice_secret_key)
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: max_gas_price,
                additional_computation_factor: 0,
            })
            .build()
            .expect("must get transaction");

    let txn = Transaction::V1(fixed_native_mint_transaction);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, Duration::from_secs(20))
        .await;

    let holds_after = fixture.check_account_balance_hold_at_tip(alice_public_key.clone());

    let current_gas_price = fixture
        .highest_complete_block()
        .maybe_current_gas_price()
        .expect("must have gas price");

    let cost = match fixture.chainspec.core_config.pricing_handling {
        PricingHandling::Classic => 0,
        PricingHandling::Fixed => {
            fixture.chainspec.system_costs_config.mint_costs().transfer * (current_gas_price as u32)
        }
    };

    assert_eq!(holds_after, holds_before + U512::from(cost));

    // Run the network at zero load and ensure the value falls back to the floor.
    for _ in 0..5 {
        let next_era = current_era.successor();
        fixture
            .run_until_stored_switch_block_header(next_era, ONE_MIN)
            .await;
        current_era = next_era;
    }

    let expected_gas_price = fixture.chainspec.vacancy_config.min_gas_price;
    let actual_gas_price = fixture.get_current_era_price();
    assert_eq!(actual_gas_price, expected_gas_price);
}

#[tokio::test]
async fn should_raise_gas_price_to_ceiling_and_reduce_to_floor_based_on_slot_utilization() {
    let scenario = GasPriceScenario::SlotUtilization;
    run_gas_price_scenario(scenario).await
}

#[tokio::test]
async fn should_raise_gas_price_to_ceiling_and_reduce_to_floor_based_on_gas_consumption() {
    let gas_limit = Chainspec::default()
        .system_costs_config
        .mint_costs()
        .transfer as u64;
    let scenario = GasPriceScenario::GasConsumptionUtilization(gas_limit);
    run_gas_price_scenario(scenario).await
}

#[tokio::test]
async fn should_raise_gas_price_to_ceiling_and_reduce_to_floor_based_on_size_consumption() {
    // The size of a native transfer is roughly 300 ~ 400 bytes
    let size_limit = 600u32;
    let scenario = GasPriceScenario::SizeUtilization(size_limit);
    run_gas_price_scenario(scenario).await
}
