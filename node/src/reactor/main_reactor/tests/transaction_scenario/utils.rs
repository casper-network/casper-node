use async_trait::async_trait;
use casper_storage::{
    data_access_layer::{
        balance::BalanceHandling,
        tagged_values::{TaggedValuesRequest, TaggedValuesResult, TaggedValuesSelection},
        BalanceRequest, BalanceResult, ProofHandling, TotalSupplyRequest, TotalSupplyResult,
    },
    global_state::state::StateProvider,
};
use casper_types::{
    account::AccountHash, bytesrepr::Bytes, testing::TestRng, EraId, ExecutionInfo, FeeHandling,
    KeyTag, PricingHandling, PricingMode, PublicKey, RefundHandling, SecretKey, TimeDiff,
    Transaction, TransactionHash, TransactionRuntimeParams, U512,
};
use once_cell::sync::OnceCell;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use crate::{
    reactor::main_reactor::tests::{
        configs_override::ConfigsOverride,
        fixture::TestFixture,
        transactions::{
            BalanceAmount, ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_PUBLIC_KEY, BOB_SECRET_KEY,
        },
        ERA_ONE, ONE_MIN, TEN_SECS,
    },
    types::transaction::transaction_v1_builder::TransactionV1Builder,
};

pub(crate) struct TestStateSnapshot {
    pub(crate) exec_infos: BTreeMap<TransactionHash, ExecutionInfo>,
    pub(crate) balances: BTreeMap<AccountHash, BalanceAmount>,
    pub(crate) total_supply: U512,
}

/// This defines the condition
/// a network should achieve after setup and start
/// before we can proceed with transaction injection
#[derive(Clone, Debug)]
pub(crate) enum RunUntilCondition {
    /// Runs the network until all nodes reach the given completed block height.
    BlockHeight { block_height: u64, within: Duration },
    /// Runs the network until all nodes' consensus components reach the given era.
    ConsensusInEra { era_id: EraId, within: Duration },
}

impl RunUntilCondition {
    async fn run_until(&self, fixture: &mut TestFixture) -> Result<(), TestScenarioError> {
        match self {
            RunUntilCondition::BlockHeight {
                block_height,
                within,
            } => {
                fixture
                    .try_run_until_block_height(*block_height, *within)
                    .await
            }
            RunUntilCondition::ConsensusInEra { era_id, within } => {
                fixture.try_until_consensus_in_era(*era_id, *within).await
            }
        }
        .map_err(|_| TestScenarioError::NetworkDidNotStabilize)
    }
}

#[derive(Debug)]
pub(crate) enum TestScenarioError {
    UnexpectedState,
    NetworkDidNotStabilize,
    CannotSetBeforeState,
}

struct ScenarioDataInstance {
    fixture: TestFixture,
    block_height: u64,
}

impl ScenarioDataInstance {
    pub(crate) async fn inject_transaction(&mut self, txn: Transaction) {
        self.fixture.inject_transaction(txn).await
    }

    pub(crate) async fn run_until_executed_transaction(
        &mut self,
        txn_hash: &TransactionHash,
        within: Duration,
    ) {
        self.fixture
            .run_until_executed_transaction(txn_hash, within)
            .await
    }
}

#[async_trait]
pub(crate) trait Assertion: Send + Sync {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>);
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum TestScenarioState {
    PreSetup,
    PreRun,
    Running,
}

pub(crate) struct TestScenario {
    state: TestScenarioState,
    data: ScenarioDataInstance,
    initial_run_until: RunUntilCondition,
    exec_infos: BTreeMap<TransactionHash, ExecutionInfo>,
    state_before_test: OnceCell<TestStateSnapshot>,
}

impl TestScenario {
    pub(crate) async fn setup(&mut self) -> Result<(), TestScenarioError> {
        if self.state != TestScenarioState::PreSetup {
            return Err(TestScenarioError::UnexpectedState);
        }
        self.run_until(self.initial_run_until.clone()).await?;
        self.state_before_test
            .set(self.get_current_state().await)
            .map_err(|_| TestScenarioError::CannotSetBeforeState)?;
        self.state = TestScenarioState::PreRun;
        Ok(())
    }

    pub(crate) async fn run(
        &mut self,
        to_inject: Vec<Transaction>,
    ) -> Result<Vec<ExecutionInfo>, TestScenarioError> {
        if self.state == TestScenarioState::PreSetup {
            return Err(TestScenarioError::UnexpectedState);
        }
        let mut to_ret = vec![];
        for transaction in &to_inject {
            let hash = transaction.hash();
            self.data.inject_transaction(transaction.clone()).await;
            self.data
                .run_until_executed_transaction(&hash, TEN_SECS)
                .await;
            let (_node_id, runner) = self.data.fixture.network.nodes().iter().next().unwrap();
            let exec_info = runner
                .main_reactor()
                .storage()
                .read_execution_info(hash)
                .expect("Expected transaction to be included in a block.");
            let transaction_block_height = exec_info.block_height;
            if transaction_block_height > self.data.block_height {
                self.data.block_height = transaction_block_height;
            }
            to_ret.push(exec_info.clone());
            self.exec_infos.insert(hash, exec_info);
        }
        self.state = TestScenarioState::Running;
        Ok(to_ret)
    }

    pub(crate) async fn run_until(
        &mut self,
        run_until: RunUntilCondition,
    ) -> Result<(), TestScenarioError> {
        run_until.run_until(&mut self.data.fixture).await
    }

    pub(crate) fn chain_name(&self) -> String {
        self.data.fixture.chainspec.network_config.name.clone()
    }

    pub(crate) async fn assert<T: Assertion>(&mut self, assertion: T) {
        if self.state_before_test.get().is_none() {
            panic!("TestScenario not in state eligible to do assertions");
        }
        let max_block_height = self.data.fixture.highest_complete_block().height();
        let mut snapshots = BTreeMap::new();
        for i in 0..=max_block_height {
            snapshots.insert(i, self.get_state_at_height(i).await);
        }
        assertion.assert(snapshots).await
    }

    async fn get_state_at_height(&self, block_height: u64) -> TestStateSnapshot {
        let all_accounts = self.get_all_accounts(block_height).await;
        let mut balances = BTreeMap::new();
        for account_hash in all_accounts {
            let balance_amount = self.get_balance_amount(account_hash, block_height).await;
            balances.insert(account_hash, balance_amount);
        }

        let total_supply = self.get_total_supply(block_height).await;
        let exec_infos: BTreeMap<TransactionHash, ExecutionInfo> = self
            .exec_infos
            .iter()
            .filter_map(|(k, v)| {
                if v.block_height <= block_height {
                    Some((*k, v.clone()))
                } else {
                    None
                }
            })
            .collect();

        TestStateSnapshot {
            exec_infos,
            balances,
            total_supply,
        }
    }

    async fn get_current_state(&self) -> TestStateSnapshot {
        let block = self.data.fixture.highest_complete_block();
        let block_height = block.height();
        self.get_state_at_height(block_height).await
    }

    async fn get_all_accounts(&self, block_height: u64) -> Vec<AccountHash> {
        let fixture = &self.data.fixture;
        let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
        let block_header = runner
            .main_reactor()
            .storage()
            .read_block_header_by_height(block_height, true)
            .expect("failure to read block header")
            .expect("should have block header");
        let state_hash = *block_header.state_root_hash();
        let request =
            TaggedValuesRequest::new(state_hash, TaggedValuesSelection::All(KeyTag::Account));
        match runner
            .main_reactor()
            .contract_runtime()
            .data_access_layer()
            .tagged_values(request)
        {
            TaggedValuesResult::Success { values, .. } => values
                .iter()
                .filter_map(|el| el.as_account().map(|el| el.account_hash()))
                .collect(),
            _ => panic!("Couldn't get all account hashes"),
        }
    }

    pub(crate) fn get_balance(
        &self,
        account_hash: AccountHash,
        block_height: Option<u64>,
        get_total: bool,
    ) -> BalanceResult {
        let fixture = &self.data.fixture;
        let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
        let protocol_version = fixture.chainspec.protocol_version();
        let block_height = block_height.unwrap_or(
            runner
                .main_reactor()
                .storage()
                .highest_complete_block_height()
                .expect("missing highest completed block"),
        );
        let block_header = runner
            .main_reactor()
            .storage()
            .read_block_header_by_height(block_height, true)
            .expect("failure to read block header")
            .unwrap();
        let state_hash = *block_header.state_root_hash();
        let balance_handling = if get_total {
            BalanceHandling::Total
        } else {
            BalanceHandling::Available
        };
        runner
            .main_reactor()
            .contract_runtime()
            .data_access_layer()
            .balance(BalanceRequest::from_account_hash(
                state_hash,
                protocol_version,
                account_hash,
                balance_handling,
                ProofHandling::NoProofs,
            ))
    }

    async fn get_balance_amount(
        &self,
        account_hash: AccountHash,
        block_height: u64,
    ) -> BalanceAmount {
        let block_height = Some(block_height);

        let total = self
            .get_balance(account_hash, block_height, true)
            .total_balance()
            .copied()
            .unwrap_or(U512::zero());
        let available = self
            .get_balance(account_hash, block_height, false)
            .available_balance()
            .copied()
            .unwrap_or(U512::zero());
        BalanceAmount { available, total }
    }

    async fn get_total_supply(&self, block_height: u64) -> U512 {
        let fixture = &self.data.fixture;
        let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
        let protocol_version = fixture.chainspec.protocol_version();
        let state_hash = *runner
            .main_reactor()
            .storage()
            .read_block_header_by_height(block_height, true)
            .expect("failure to read block header")
            .unwrap()
            .state_root_hash();

        let total_supply_req = TotalSupplyRequest::new(state_hash, protocol_version);
        let result = runner
            .main_reactor()
            .contract_runtime()
            .data_access_layer()
            .total_supply(total_supply_req);

        if let TotalSupplyResult::Success { total_supply } = result {
            total_supply
        } else {
            panic!("Can't get total supply")
        }
    }

    pub(crate) fn mint_const_transfer_cost(&self) -> u32 {
        self.data
            .fixture
            .chainspec
            .system_costs_config
            .mint_costs()
            .transfer
    }

    pub(crate) fn native_transfer_minimum_motes(&self) -> u64 {
        self.data
            .fixture
            .chainspec
            .transaction_config
            .native_transfer_minimum_motes
    }

    pub(crate) fn get_gas_limit_for_lane(&self, lane_id: u8) -> Option<u64> {
        self.data
            .fixture
            .chainspec
            .transaction_config
            .transaction_v1_config
            .get_lane_by_id(lane_id)
            .map(|el| el.max_transaction_gas_limit)
    }

    pub(crate) fn get_block_height(&self) -> u64 {
        self.data.block_height
    }
}

type StakesType = Option<(Vec<Arc<SecretKey>>, BTreeMap<PublicKey, (U512, U512)>)>;

#[derive(Default)]
pub(crate) struct TestScenarioBuilder {
    maybe_stakes_setup: StakesType,
    maybe_pricing_handling: Option<PricingHandling>,
    maybe_initial_run_until: Option<RunUntilCondition>,
    maybe_refund_handling: Option<RefundHandling>,
    maybe_fee_handling: Option<FeeHandling>,
    maybe_balance_hold_interval_override: Option<TimeDiff>,
    maybe_minimum_era_height: Option<u64>,
}

impl TestScenarioBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn build(self, rng: &mut TestRng) -> TestScenario {
        let TestScenarioBuilder {
            maybe_stakes_setup,
            maybe_pricing_handling,
            maybe_initial_run_until,
            maybe_refund_handling,
            maybe_fee_handling,
            maybe_balance_hold_interval_override,
            maybe_minimum_era_height,
        } = self;
        let (secret_keys, stakes) = maybe_stakes_setup.unwrap_or({
            /* Node 0 is effectively guaranteed to be the proposer. */
            let stakes: BTreeMap<PublicKey, (U512, U512)> = vec![
                (
                    ALICE_PUBLIC_KEY.clone(),
                    (U512::from(u64::MAX), U512::from(u128::MAX)),
                ),
                (
                    BOB_PUBLIC_KEY.clone(),
                    (U512::from(u64::MAX), U512::from(1)),
                ),
            ]
            .into_iter()
            .collect();
            let secret_keys = vec![ALICE_SECRET_KEY.clone(), BOB_SECRET_KEY.clone()];
            (secret_keys, stakes)
        });

        let pricing_handling = maybe_pricing_handling.unwrap_or(PricingHandling::Fixed);
        let initial_run_until =
            maybe_initial_run_until.unwrap_or(RunUntilCondition::ConsensusInEra {
                era_id: ERA_ONE,
                within: ONE_MIN,
            });
        let config = ConfigsOverride::default().with_pricing_handling(pricing_handling);
        let config = if let Some(refund_handling) = maybe_refund_handling {
            config.with_refund_handling(refund_handling)
        } else {
            config
        };
        let config = if let Some(fee_handling) = maybe_fee_handling {
            config.with_fee_handling(fee_handling)
        } else {
            config
        };
        let config =
            if let Some(balance_hold_interval_override) = maybe_balance_hold_interval_override {
                config.with_balance_hold_interval(balance_hold_interval_override)
            } else {
                config
            };
        let config = if let Some(minimum_era_height) = maybe_minimum_era_height {
            config.with_minimum_era_height(minimum_era_height)
        } else {
            config
        };
        let child_rng = rng.create_child();
        let fixture =
            TestFixture::new_with_keys(child_rng, secret_keys, stakes, Some(config)).await;
        let data = ScenarioDataInstance {
            fixture,
            block_height: 0_u64,
        };

        TestScenario {
            state: TestScenarioState::PreSetup,
            data,
            initial_run_until,
            exec_infos: BTreeMap::new(),
            state_before_test: OnceCell::new(),
        }
    }

    /// Sets refund handling config option.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.maybe_refund_handling = Some(refund_handling);
        self
    }

    pub(crate) fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.maybe_fee_handling = Some(fee_handling);
        self
    }

    pub(crate) fn with_balance_hold_interval(mut self, balance_hold_interval: TimeDiff) -> Self {
        self.maybe_balance_hold_interval_override = Some(balance_hold_interval);
        self
    }

    pub(crate) fn with_minimum_era_height(mut self, minimum_era_height: u64) -> Self {
        self.maybe_minimum_era_height = Some(minimum_era_height);
        self
    }
}

pub(super) fn build_wasm_transction(
    chain_name: String,
    from: &SecretKey,
    pricing: PricingMode,
) -> Transaction {
    //These bytes are intentionally so large - this way they fall into "WASM_LARGE" category in the
    // local chainspec Alternatively we could change the chainspec to have a different limits
    // for the wasm categories, but that would require aligning all tests that use local
    // chainspec
    let module_bytes = Bytes::from(vec![1; 172_033]);
    Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(chain_name)
        .with_pricing_mode(pricing)
        .with_initiator_addr(PublicKey::from(from))
        .build()
        .unwrap(),
    )
}
