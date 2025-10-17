use itertools::Itertools;
use std::{
    collections::BTreeMap, convert::TryFrom, iter, net::SocketAddr, str::FromStr, sync::Arc,
    time::Duration,
};

use num_rational::Ratio;
use num_traits::Zero;
use rand::Rng;
use tempfile::TempDir;
use tokio::time::error::Elapsed;
use tracing::info;

use casper_storage::{
    data_access_layer::{
        balance::{BalanceHandling, BalanceResult},
        BalanceRequest, BidsRequest, BidsResult, ProofHandling,
    },
    global_state::state::{StateProvider, StateReader},
};
use casper_types::{
    execution::{ExecutionResult, TransformV2},
    system::auction::{DelegationRate, DelegatorKind},
    testing::TestRng,
    AccountConfig, AccountsConfig, ActivationPoint, AddressableEntityHash, Block, BlockBody,
    BlockHash, BlockV2, CLValue, Chainspec, ChainspecRawBytes, EraEnd, EraId, Key, Motes,
    NextUpgrade, ProtocolVersion, PublicKey, SecretKey, StoredValue, SystemHashRegistry, TimeDiff,
    Timestamp, Transaction, TransactionHash, ValidatorConfig, U512,
};

use crate::{
    components::{gossiper, network, storage},
    effect::EffectExt,
    reactor::main_reactor::{
        tests::{
            configs_override::{ConfigsOverride, NodeConfigOverride},
            initial_stakes::InitialStakes,
            Nodes, ERA_TWO,
        },
        Config, MainReactor, ReactorState,
    },
    testing::{self, filter_reactor::FilterReactor, network::TestingNetwork},
    types::NodeId,
    utils::{External, Loadable, Source, RESOURCES_PATH},
    WithDir,
};

pub(crate) struct NodeContext {
    pub id: NodeId,
    pub secret_key: Arc<SecretKey>,
    pub config: Config,
    pub storage_dir: TempDir,
}

pub(crate) struct TestFixture {
    pub rng: TestRng,
    pub node_contexts: Vec<NodeContext>,
    pub network: TestingNetwork<FilterReactor<MainReactor>>,
    pub chainspec: Arc<Chainspec>,
    pub chainspec_raw_bytes: Arc<ChainspecRawBytes>,
}

impl TestFixture {
    /// Sets up a new fixture with the number of nodes indicated by `initial_stakes`.
    ///
    /// Runs the network until all nodes are initialized (i.e. none of their reactor states are
    /// still `ReactorState::Initialize`).
    pub(crate) async fn new(
        initial_stakes: InitialStakes,
        spec_override: Option<ConfigsOverride>,
    ) -> Self {
        let rng = TestRng::new();
        Self::new_with_rng(initial_stakes, spec_override, rng).await
    }

    pub(crate) async fn new_with_rng(
        initial_stakes: InitialStakes,
        spec_override: Option<ConfigsOverride>,
        mut rng: TestRng,
    ) -> Self {
        let stake_values = match initial_stakes {
            InitialStakes::FromVec(stakes) => {
                stakes.into_iter().map(|stake| stake.into()).collect()
            }
            InitialStakes::Random { count } => {
                // By default, we use very large stakes so we would catch overflow issues.
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
            .map(|(secret_key, stake)| {
                (
                    PublicKey::from(secret_key.as_ref()),
                    (U512::from(100_000_000_000_000_000u64), stake),
                )
            })
            .collect();

        Self::new_with_keys(rng, secret_keys, stakes, spec_override).await
    }

    pub(crate) async fn new_with_keys(
        rng: TestRng,
        secret_keys: Vec<Arc<SecretKey>>,
        stakes: BTreeMap<PublicKey, (U512, U512)>,
        spec_override: Option<ConfigsOverride>,
    ) -> Self {
        testing::init_logging();

        // Load the `local` chainspec.
        let (mut chainspec, chainspec_raw_bytes) =
            <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        // Override accounts with those generated from the keys.
        let accounts = stakes
            .into_iter()
            .map(|(public_key, (balance, bonded_amount))| {
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
            vm_casper_v2,
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
        chainspec.transaction_config.runtime_config.vm_casper_v2 = vm_casper_v2;
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
    pub(crate) fn rng_mut(&mut self) -> &mut TestRng {
        &mut self.rng
    }

    /// Returns the highest complete block from node 0.
    ///
    /// Panics if there is no such block.
    #[track_caller]
    pub(crate) fn highest_complete_block(&self) -> Block {
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
    pub(crate) fn get_block_by_height(&self, block_height: u64) -> Block {
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
    pub(crate) fn get_block_gas_price_by_public_key(
        &self,
        maybe_public_key: Option<&PublicKey>,
    ) -> u8 {
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
    pub(crate) fn switch_block(&self, era: EraId) -> BlockV2 {
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
    pub(crate) fn create_node_config(
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
                sandboxed_execution_allowed_ips: vec!["127.0.0.1".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let NodeConfigOverride {
            sync_handling_override,
            idle_tolerance,
        } = node_config_override;
        if let Some(sync_handling) = sync_handling_override {
            cfg.node.sync_handling = sync_handling;
        }
        if let Some(idle) = idle_tolerance {
            cfg.node.idle_tolerance = idle
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
    pub(crate) async fn add_node(
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
    pub(crate) fn remove_and_stop_node(&mut self, index: usize) -> NodeContext {
        let node_context = self.node_contexts.remove(index);
        let runner = self.network.remove_node(&node_context.id).unwrap();
        runner.is_shutting_down.set();
        info!("removed node {} with id {}", index, node_context.id);
        node_context
    }

    /// Runs the network until `condition` is true.
    ///
    /// Returns an error if the condition isn't met in time.
    pub(crate) async fn try_run_until<F>(
        &mut self,
        condition: F,
        within: Duration,
    ) -> Result<(), Elapsed>
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
    pub(crate) async fn run_until<F>(&mut self, condition: F, within: Duration)
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
    pub(crate) async fn try_run_until_block_height(
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
    pub(crate) async fn run_until_block_height(&mut self, block_height: u64, within: Duration) {
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
    pub(crate) async fn run_until_consensus_in_era(&mut self, era_id: EraId, within: Duration) {
        self.try_until_consensus_in_era(era_id, within)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "should reach {} within {} seconds",
                    era_id,
                    within.as_secs_f64(),
                )
            })
    }

    /// Runs the network until all nodes' consensus components reach the given era.
    pub(crate) async fn try_until_consensus_in_era(
        &mut self,
        era_id: EraId,
        within: Duration,
    ) -> Result<(), Elapsed> {
        self.try_run_until(
            move |nodes: &Nodes| {
                nodes
                    .values()
                    .all(|runner| runner.main_reactor().consensus().current_era() == Some(era_id))
            },
            within,
        )
        .await
    }

    /// Runs the network until all nodes' storage components have stored the switch block header for
    /// the given era.
    ///
    /// Panics if the condition isn't met in time.
    pub(crate) async fn run_until_stored_switch_block_header(
        &mut self,
        era_id: EraId,
        within: Duration,
    ) {
        self.try_until_stored_switch_block_header(era_id, within)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "should have stored switch block header for {} within {} seconds",
                    era_id,
                    within.as_secs_f64(),
                )
            })
    }

    /// Runs the network until all nodes' storage components have stored the switch block header for
    /// the given era.
    pub(crate) async fn try_until_stored_switch_block_header(
        &mut self,
        era_id: EraId,
        within: Duration,
    ) -> Result<(), Elapsed> {
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
                        .is_some_and(|header| {
                            header.era_id() == era_id
                                && available_block_range.contains(header.height())
                        })
                })
            },
            within,
        )
        .await
    }

    /// Runs the network until all nodes have executed the given transaction and stored the
    /// execution result.
    ///
    /// Panics if the condition isn't met in time.
    pub(crate) async fn run_until_executed_transaction(
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

    pub(crate) async fn schedule_upgrade_for_era_two(&mut self) {
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
    pub(crate) fn check_bid_existence_at_tip(
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
            .read_highest_block_with_signatures(true)
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
    pub(crate) fn system_contract_hash(&self, system_contract_name: &str) -> AddressableEntityHash {
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
    pub(crate) fn get_current_era_price(&self) -> u8 {
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
    pub(crate) fn check_account_balance_hold_at_tip(&self, account_public_key: PublicKey) -> U512 {
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

    pub(crate) async fn inject_transaction(&mut self, txn: Transaction) {
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
    pub(crate) fn successful_execution_transforms(
        &self,
        txn_hash: &TransactionHash,
    ) -> Vec<TransformV2> {
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
            ExecutionResult::V2(execution_result_v2) => {
                if execution_result_v2.error_message.is_none() {
                    execution_result_v2.effects.transforms().to_vec()
                } else {
                    panic!(
                        "transaction execution failed: {:?} gas: {}",
                        execution_result_v2.error_message, execution_result_v2.consumed
                    );
                }
            }
        }
    }

    /// Returns the execution results from storage.
    /// Panics on error.
    #[track_caller]
    pub(crate) fn transaction_execution_result(
        &self,
        txn_hash: &TransactionHash,
    ) -> ExecutionResult {
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
            .read_execution_result(txn_hash)
            .expect("node 0 should have given execution result")
    }

    pub(crate) fn delete_block_utilization_score_by_block_hash_in_node(
        &mut self,
        node_public_key: &PublicKey,
        block_hash: BlockHash,
    ) {
        let (_, runner) = self
            .network
            .nodes_mut()
            .iter_mut()
            .find(|(_, runner)| runner.main_reactor().consensus.public_key() == node_public_key)
            .expect("should have runner");

        runner
            .main_reactor_as_mut()
            .storage
            .delete_block_utilization_score_by_block_hash(block_hash)
    }

    pub(crate) async fn check_gas_price_for_nodes(
        &mut self,
        expected_gas_price: u8,
        within: Duration,
    ) {
        self.try_run_until(
            move |nodes| {
                nodes.values().all(|runner| {
                    let era_end = runner
                        .main_reactor()
                        .storage()
                        .read_highest_switch_block_headers(1)
                        .unwrap()
                        .last()
                        .expect("must have block header")
                        .clone_era_end()
                        .expect("must have era end for switch block");

                    if let EraEnd::V2(era_end) = era_end {
                        era_end.next_era_gas_price() == expected_gas_price
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
                "should have same gas price across all nodes within {} seconds",
                within.as_secs_f64(),
            )
        })
    }

    #[inline(always)]
    pub(crate) fn network_mut(&mut self) -> &mut TestingNetwork<FilterReactor<MainReactor>> {
        &mut self.network
    }

    pub(crate) fn run_until_stopped(
        self,
        rng: TestRng,
    ) -> impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)> {
        self.network.crank_until_stopped(rng)
    }

    /// Runs the network until all nodes have executed the given transaction and stored the
    /// execution result.
    ///
    /// Panics if the condition isn't met in time.
    pub(crate) async fn assert_execution_in_lane(
        &mut self,
        txn_hash: &TransactionHash,
        lane_id: u8,
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
                            if let BlockBody::V2(v2_body) = runner
                                .main_reactor()
                                .storage()
                                .read_block_by_height(exec_info.block_height)
                                .unwrap()
                                .take_body()
                            {
                                v2_body.transactions_by_lane_id(lane_id).contains(txn_hash)
                            } else {
                                false
                            }
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
}

pub(crate) fn standard_stakes(
    alice_public_key: PublicKey,
    bob_public_key: PublicKey,
    charlie_public_key: Option<PublicKey>,
) -> BTreeMap<PublicKey, (U512, U512)> {
    let mut ret = BTreeMap::new();
    ret.insert(
        alice_public_key.clone(),
        (
            U512::from(100_000_000_000_000_000u64),
            U512::from(u128::MAX),
        ),
    );
    ret.insert(
        bob_public_key.clone(),
        (U512::from(100_000_000_000_000_000u64), U512::from(1)),
    );

    if let Some(pub_k) = charlie_public_key {
        ret.insert(pub_k, (U512::from(u32::MAX - 1), U512::from(1)));
    }
    ret
}
