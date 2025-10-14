use super::{fixture::TestFixture, *};
use crate::{
    testing::LARGE_WASM_LANE_ID,
    types::{transaction::calculate_transaction_lane_for_transaction, MetaTransaction},
};
use casper_storage::data_access_layer::{
    AddressableEntityRequest, BalanceIdentifier, BalanceIdentifierPurseRequest,
    BalanceIdentifierPurseResult, ProofHandling, QueryRequest, QueryResult,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::NamedKeyAddr,
    runtime_args,
    system::mint::{ARG_AMOUNT, ARG_TARGET},
    AccessRights, AddressableEntity, Digest, EntityAddr, ExecutableDeployItem, ExecutionInfo,
    TransactionRuntimeParams, URef, URefAddr, DEFAULT_TRANSFER_COST,
};
use once_cell::sync::Lazy;
use std::collections::BTreeMap;

use crate::reactor::main_reactor::tests::{
    configs_override::ConfigsOverride, fixture::standard_stakes, initial_stakes::InitialStakes,
};
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    execution::ExecutionResultV1,
};

pub(crate) static ALICE_SECRET_KEY: Lazy<Arc<SecretKey>> = Lazy::new(|| {
    Arc::new(SecretKey::ed25519_from_bytes([0xAA; SecretKey::ED25519_LENGTH]).unwrap())
});
pub(crate) static ALICE_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ALICE_SECRET_KEY.clone()));

pub(crate) static BOB_SECRET_KEY: Lazy<Arc<SecretKey>> = Lazy::new(|| {
    Arc::new(SecretKey::ed25519_from_bytes([0xBB; SecretKey::ED25519_LENGTH]).unwrap())
});
pub(crate) static BOB_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*BOB_SECRET_KEY.clone()));

pub(crate) static CHARLIE_SECRET_KEY: Lazy<Arc<SecretKey>> = Lazy::new(|| {
    Arc::new(SecretKey::ed25519_from_bytes([0xCC; SecretKey::ED25519_LENGTH]).unwrap())
});
pub(crate) static CHARLIE_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*CHARLIE_SECRET_KEY.clone()));

// The amount of gas it takes to execute the generated do_nothing.wasm.
// Passing this around as a constant is brittle and should be replaced
// with a more sustainable solution in the future.
const DO_NOTHING_WASM_EXECUTION_GAS: u64 = 117720_u64;
pub(crate) const MIN_GAS_PRICE: u8 = 1;
const CHAIN_NAME: &str = "single-transaction-test-net";

struct SingleTransactionTestCase {
    fixture: TestFixture,
    alice_public_key: PublicKey,
    bob_public_key: PublicKey,
    charlie_public_key: PublicKey,
}

#[derive(Debug, PartialEq)]
pub(crate) struct BalanceAmount {
    pub(crate) available: U512,
    pub(crate) total: U512,
}

impl BalanceAmount {
    pub(crate) fn zero() -> Self {
        Self {
            available: U512::zero(),
            total: U512::zero(),
        }
    }
}

impl SingleTransactionTestCase {
    fn default_test_config() -> ConfigsOverride {
        ConfigsOverride::default()
            .with_minimum_era_height(5) // make the era longer so that the transaction doesn't land in the switch block.
            .with_balance_hold_interval(TimeDiff::from_seconds(5))
            .with_chain_name("single-transaction-test-net".to_string())
    }

    async fn new(
        alice_secret_key: Arc<SecretKey>,
        bob_secret_key: Arc<SecretKey>,
        charlie_secret_key: Arc<SecretKey>,
        network_config: Option<ConfigsOverride>,
    ) -> Self {
        let rng = TestRng::new();

        let alice_public_key = PublicKey::from(&*alice_secret_key);
        let bob_public_key = PublicKey::from(&*bob_secret_key);
        let charlie_public_key = PublicKey::from(&*charlie_secret_key);

        let stakes = standard_stakes(
            alice_public_key.clone(),
            bob_public_key.clone(),
            Some(charlie_public_key.clone()),
        );

        let fixture = TestFixture::new_with_keys(
            rng,
            vec![alice_secret_key.clone(), bob_secret_key.clone()],
            stakes,
            network_config,
        )
        .await;
        Self {
            fixture,
            alice_public_key,
            bob_public_key,
            charlie_public_key,
        }
    }

    async fn new_with_stakes(
        alice_secret_key: Arc<SecretKey>,
        bob_secret_key: Arc<SecretKey>,
        charlie_secret_key: Arc<SecretKey>,
        network_config: Option<ConfigsOverride>,
        stakes: BTreeMap<PublicKey, (U512, U512)>,
    ) -> Self {
        let rng = TestRng::new();

        let alice_public_key = PublicKey::from(&*alice_secret_key);
        let bob_public_key = PublicKey::from(&*bob_secret_key);
        let charlie_public_key = PublicKey::from(&*charlie_secret_key);

        let fixture = TestFixture::new_with_keys(
            rng,
            vec![alice_secret_key.clone(), bob_secret_key.clone()],
            stakes,
            network_config,
        )
        .await;
        Self {
            fixture,
            alice_public_key,
            bob_public_key,
            charlie_public_key,
        }
    }

    fn chainspec(&self) -> &Chainspec {
        &self.fixture.chainspec
    }

    fn get_balances(
        &mut self,
        block_height: Option<u64>,
    ) -> (BalanceAmount, BalanceAmount, Option<BalanceAmount>) {
        let alice_total_balance =
            *get_balance(&self.fixture, &self.alice_public_key, block_height, true)
                .total_balance()
                .expect("Expected Alice to have a balance.");
        let bob_total_balance =
            *get_balance(&self.fixture, &self.bob_public_key, block_height, true)
                .total_balance()
                .expect("Expected Bob to have a balance.");

        let alice_available_balance =
            *get_balance(&self.fixture, &self.alice_public_key, block_height, false)
                .available_balance()
                .expect("Expected Alice to have a balance.");
        let bob_available_balance =
            *get_balance(&self.fixture, &self.bob_public_key, block_height, false)
                .available_balance()
                .expect("Expected Bob to have a balance.");

        let charlie_available_balance =
            get_balance(&self.fixture, &self.charlie_public_key, block_height, false)
                .available_balance()
                .copied();

        let charlie_total_balance =
            get_balance(&self.fixture, &self.charlie_public_key, block_height, true)
                .available_balance()
                .copied();

        let charlie_amount = charlie_available_balance.map(|avail_balance| BalanceAmount {
            available: avail_balance,
            total: charlie_total_balance.unwrap(),
        });

        (
            BalanceAmount {
                available: alice_available_balance,
                total: alice_total_balance,
            },
            BalanceAmount {
                available: bob_available_balance,
                total: bob_total_balance,
            },
            charlie_amount,
        )
    }

    async fn send_transaction(
        &mut self,
        txn: Transaction,
    ) -> (TransactionHash, u64, ExecutionResult) {
        let txn_hash = txn.hash();

        self.fixture.inject_transaction(txn).await;
        self.fixture
            .run_until_executed_transaction(&txn_hash, Duration::from_secs(30))
            .await;

        let (_node_id, runner) = self.fixture.network.nodes().iter().next().unwrap();
        let exec_info = runner
            .main_reactor()
            .storage()
            .read_execution_info(txn_hash)
            .expect("Expected transaction to be included in a block.");

        (
            txn_hash,
            exec_info.block_height,
            exec_info
                .execution_result
                .expect("Exec result should have been stored."),
        )
    }

    fn get_total_supply(&mut self, block_height: Option<u64>) -> U512 {
        let (_node_id, runner) = self.fixture.network.nodes().iter().next().unwrap();
        let protocol_version = self.fixture.chainspec.protocol_version();
        let height = block_height.unwrap_or(
            runner
                .main_reactor()
                .storage()
                .highest_complete_block_height()
                .expect("missing highest completed block"),
        );
        let state_hash = *runner
            .main_reactor()
            .storage()
            .read_block_header_by_height(height, true)
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

    fn get_accumulate_purse_balance(
        &mut self,
        block_height: Option<u64>,
        get_total: bool,
    ) -> BalanceResult {
        let (_node_id, runner) = self.fixture.network.nodes().iter().next().unwrap();
        let protocol_version = self.fixture.chainspec.protocol_version();
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
            .balance(BalanceRequest::new(
                state_hash,
                protocol_version,
                BalanceIdentifier::Accumulate,
                balance_handling,
                ProofHandling::NoProofs,
            ))
    }
}

async fn transfer_to_account<A: Into<U512>>(
    fixture: &mut TestFixture,
    amount: A,
    from: &SecretKey,
    to: PublicKey,
    pricing: PricingMode,
    transfer_id: Option<u64>,
) -> (TransactionHash, u64, ExecutionResult) {
    let chain_name = fixture.chainspec.network_config.name.clone();

    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(amount, None, to, transfer_id)
            .unwrap()
            .with_initiator_addr(PublicKey::from(from))
            .with_pricing_mode(pricing)
            .with_chain_name(chain_name)
            .build()
            .unwrap(),
    );

    txn.sign(from);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;

    info!("transfer_to_account starting run_until_executed_transaction");
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    info!("transfer_to_account finished run_until_executed_transaction");
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let exec_info = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_hash)
        .expect("Expected transaction to be included in a block.");

    (
        txn_hash,
        exec_info.block_height,
        exec_info
            .execution_result
            .expect("Exec result should have been stored."),
    )
}

async fn send_add_bid<A: Into<U512>>(
    fixture: &mut TestFixture,
    amount: A,
    signing_key: &SecretKey,
    pricing: PricingMode,
) -> (TransactionHash, u64, ExecutionResult) {
    let chain_name = fixture.chainspec.network_config.name.clone();
    let public_key = PublicKey::from(signing_key);

    let mut txn = Transaction::from(
        TransactionV1Builder::new_add_bid(public_key.clone(), 10, amount, None, None, None)
            .unwrap()
            .with_initiator_addr(public_key)
            .with_pricing_mode(pricing)
            .with_chain_name(chain_name)
            .build()
            .unwrap(),
    );

    txn.sign(signing_key);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;

    info!("transfer_to_account starting run_until_executed_transaction");
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    info!("transfer_to_account finished run_until_executed_transaction");
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let exec_info = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_hash)
        .expect("Expected transaction to be included in a block.");

    (
        txn_hash,
        exec_info.block_height,
        exec_info
            .execution_result
            .expect("Exec result should have been stored."),
    )
}

async fn send_wasm_transaction(
    fixture: &mut TestFixture,
    from: &SecretKey,
    pricing: PricingMode,
) -> (TransactionHash, u64, ExecutionResult) {
    let chain_name = fixture.chainspec.network_config.name.clone();

    //These bytes are intentionally so large - this way they fall into "WASM_LARGE" category in the
    // local chainspec Alternatively we could change the chainspec to have a different limits
    // for the wasm categories, but that would require aligning all tests that use local
    // chainspec
    let module_bytes = Bytes::from(vec![1; 172_033]);
    let mut txn = Transaction::from(
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
    );

    txn.sign(from);
    let txn_hash = txn.hash();

    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, TEN_SECS)
        .await;

    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let exec_info = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_hash)
        .expect("Expected transaction to be included in a block.");

    (
        txn_hash,
        exec_info.block_height,
        exec_info
            .execution_result
            .expect("Exec result should have been stored."),
    )
}

fn get_main_purse(fixture: &mut TestFixture, account_key: &PublicKey) -> Result<URefAddr, ()> {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let block_height = runner
        .main_reactor()
        .storage()
        .highest_complete_block_height()
        .expect("missing highest completed block");
    let block_header = runner
        .main_reactor()
        .storage()
        .read_block_header_by_height(block_height, true)
        .expect("failure to read block header")
        .unwrap();
    let state_hash = *block_header.state_root_hash();
    let protocol_version = fixture.chainspec.protocol_version();
    let identifier = BalanceIdentifier::Account(account_key.to_account_hash());
    let request = BalanceIdentifierPurseRequest::new(state_hash, protocol_version, identifier);
    match runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .balance_purse(request)
    {
        BalanceIdentifierPurseResult::Success { purse_addr } => Ok(purse_addr),
        BalanceIdentifierPurseResult::RootNotFound | BalanceIdentifierPurseResult::Failure(_) => {
            Err(())
        }
    }
}

pub(crate) fn get_balance(
    fixture: &TestFixture,
    account_key: &PublicKey,
    block_height: Option<u64>,
    get_total: bool,
) -> BalanceResult {
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
        .expect("should have header");
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
        .balance(BalanceRequest::from_public_key(
            state_hash,
            protocol_version,
            account_key.clone(),
            balance_handling,
            ProofHandling::NoProofs,
        ))
}

fn get_bids(fixture: &mut TestFixture, block_height: Option<u64>) -> Option<Vec<BidKind>> {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
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

    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .bids(BidsRequest::new(state_hash))
        .into_option()
}

fn get_payment_purse_balance(
    fixture: &mut TestFixture,
    block_height: Option<u64>,
) -> BalanceResult {
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
    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .balance(BalanceRequest::new(
            state_hash,
            protocol_version,
            BalanceIdentifier::Payment,
            BalanceHandling::Available,
            ProofHandling::NoProofs,
        ))
}

fn get_entity_addr_from_account_hash(
    fixture: &mut TestFixture,
    state_root_hash: Digest,
    account_hash: AccountHash,
) -> EntityAddr {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let result = match runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .query(QueryRequest::new(
            state_root_hash,
            Key::Account(account_hash),
            vec![],
        )) {
        QueryResult::Success { value, .. } => value,
        err => panic!("Expected QueryResult::Success but got {:?}", err),
    };

    let key = if fixture.chainspec.core_config.addressable_entity_enabled {
        result
            .as_cl_value()
            .expect("should have a CLValue")
            .to_t::<Key>()
            .expect("should have a Key")
    } else {
        result.as_account().expect("must have account");
        Key::Account(account_hash)
    };

    match key {
        Key::Account(account_has) => EntityAddr::Account(account_has.value()),
        Key::Hash(hash) => EntityAddr::SmartContract(hash),
        Key::AddressableEntity(addr) => addr,
        _ => panic!("unexpected key"),
    }
}

fn get_entity(
    fixture: &mut TestFixture,
    state_root_hash: Digest,
    entity_addr: EntityAddr,
) -> AddressableEntity {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let (key, is_contract) = if fixture.chainspec.core_config.addressable_entity_enabled {
        (Key::AddressableEntity(entity_addr), false)
    } else {
        match entity_addr {
            EntityAddr::System(hash)
            | EntityAddr::SmartContract(hash)
            | EntityAddr::Package(hash) => (Key::Hash(hash), true),
            EntityAddr::Account(hash) => (Key::Account(AccountHash::new(hash)), false),
        }
    };

    let result = match runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .query(QueryRequest::new(state_root_hash, key, vec![]))
    {
        QueryResult::Success { value, .. } => value,
        err => panic!("Expected QueryResult::Success but got {:?}", err),
    };

    if fixture.chainspec.core_config.addressable_entity_enabled {
        result
            .into_addressable_entity()
            .expect("should have an AddressableEntity")
    } else if is_contract {
        AddressableEntity::from(result.as_contract().expect("must have contract").clone())
    } else {
        AddressableEntity::from(result.as_account().expect("must have account").clone())
    }
}

fn get_entity_named_key(
    fixture: &mut TestFixture,
    state_root_hash: Digest,
    entity_addr: EntityAddr,
    named_key: &str,
) -> Option<Key> {
    if fixture.chainspec.core_config.addressable_entity_enabled {
        let key = if let EntityAddr::Package(hash) = entity_addr {
            let key = Key::Package(hash.into());
            match query_global_state(fixture, state_root_hash, key) {
                Some(val) => match &*val {
                    StoredValue::SmartContract(package) => {
                        let entity_addr = *package
                            .versions()
                            .latest()
                            .expect("must have at least one active version");
                        Key::NamedKey(
                            NamedKeyAddr::new_from_string(entity_addr, named_key.to_owned())
                                .expect("should be valid NamedKeyAddr"),
                        )
                    }
                    value => panic!("Expected Package but got {:?}", value),
                },
                None => return None,
            }
        } else {
            Key::NamedKey(
                NamedKeyAddr::new_from_string(entity_addr, named_key.to_owned())
                    .expect("should be valid NamedKeyAddr"),
            )
        };

        match query_global_state(fixture, state_root_hash, key) {
            Some(val) => match &*val {
                StoredValue::NamedKey(named_key) => {
                    Some(named_key.get_key().expect("should have a Key"))
                }
                value => panic!("Expected NamedKey but got {:?}", value),
            },
            None => None,
        }
    } else {
        match entity_addr {
            EntityAddr::System(hash)
            | EntityAddr::SmartContract(hash)
            | EntityAddr::Package(hash) => {
                match query_global_state(fixture, state_root_hash, Key::Hash(hash)) {
                    Some(val) => match &*val {
                        StoredValue::Contract(contract) => {
                            contract.named_keys().get(named_key).copied()
                        }
                        value => panic!("Expected Contract but got {:?}", value),
                    },
                    None => None,
                }
            }
            EntityAddr::Account(hash) => {
                match query_global_state(
                    fixture,
                    state_root_hash,
                    Key::Account(AccountHash::new(hash)),
                ) {
                    Some(val) => match &*val {
                        StoredValue::Account(account) => {
                            account.named_keys().get(named_key).copied()
                        }
                        value => panic!("Expected Account but got {:?}", value),
                    },
                    None => None,
                }
            }
        }
    }
}

fn query_global_state(
    fixture: &mut TestFixture,
    state_root_hash: Digest,
    key: Key,
) -> Option<Box<StoredValue>> {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    match runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .query(QueryRequest::new(state_root_hash, key, vec![]))
    {
        QueryResult::Success { value, .. } => Some(value),
        _err => None,
    }
}

fn get_entity_by_account_hash(
    fixture: &mut TestFixture,
    state_root_hash: Digest,
    account_hash: AccountHash,
) -> AddressableEntity {
    let (_node_id, runner) = fixture.network.nodes().iter().next().unwrap();
    let key = if fixture.chainspec.core_config.addressable_entity_enabled {
        Key::AddressableEntity(EntityAddr::Account(account_hash.value()))
    } else {
        Key::Account(account_hash)
    };
    runner
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .addressable_entity(AddressableEntityRequest::new(state_root_hash, key))
        .into_option()
        .unwrap_or_else(|| {
            panic!(
                "Expected to find an entity: root_hash {:?}, account hash {:?}",
                state_root_hash, account_hash
            )
        })
}

pub(crate) fn assert_exec_result_cost(
    exec_result: ExecutionResult,
    expected_cost: U512,
    expected_consumed_gas: Gas,
    msg: &str,
) {
    match exec_result {
        ExecutionResult::V2(exec_result_v2) => {
            assert_eq!(exec_result_v2.cost, expected_cost, "{} cost", msg);
            assert_eq!(
                exec_result_v2.consumed, expected_consumed_gas,
                "{} consumed",
                msg
            );
        }
        _ => {
            panic!("Unexpected exec result version.")
        }
    }
}

// Returns `true` is the execution result is a success.
pub fn exec_result_is_success(exec_result: &ExecutionResult) -> bool {
    match exec_result {
        ExecutionResult::V2(execution_result_v2) => execution_result_v2.error_message.is_none(),
        ExecutionResult::V1(ExecutionResultV1::Success { .. }) => true,
        ExecutionResult::V1(ExecutionResultV1::Failure { .. }) => false,
    }
}

#[tokio::test]
async fn should_accept_transfer_without_id() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default().with_pricing_handling(PricingHandling::Fixed);
    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;
    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let (_, _, result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        },
        None,
    )
    .await;

    assert!(exec_result_is_success(&result))
}

#[tokio::test]
async fn should_native_transfer_nofee_norefund_fixed() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&fixture, &alice_public_key, None, true)
        .available_balance()
        .expect("Expected Alice to have a balance.");

    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        TRANSFER_AMOUNT,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        },
        Some(0xDEADBEEF),
    )
    .await;

    let expected_transfer_gas = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas; // since we set gas_price_tolerance to 1.

    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost,
        Gas::new(expected_transfer_gas),
        "transfer_cost_fixed_price_no_fee_no_refund",
    );

    let alice_available_balance =
        get_balance(&fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance = get_balance(&fixture, &alice_public_key, Some(block_height), true);

    // since FeeHandling is set to NoFee, we expect that there's a hold on Alice's balance for the
    // cost of the transfer. The total balance of Alice now should be the initial balance - the
    // amount transferred to Charlie.
    let alice_expected_total_balance = alice_initial_balance - TRANSFER_AMOUNT;
    // The available balance is the initial balance - the amount transferred to Charlie - the hold
    // for the transfer cost.
    let alice_expected_available_balance = alice_expected_total_balance - expected_transfer_cost;

    assert_eq!(
        alice_total_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    let charlie_balance = get_balance(&fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .available_balance()
            .expect("Expected Charlie to have a balance")
            .clone(),
        TRANSFER_AMOUNT.into()
    );

    // Check if the hold is released.
    let hold_release_block_height = block_height + 8; // Block time is 1s.
    fixture
        .run_until_block_height(hold_release_block_height, ONE_MIN)
        .await;

    let alice_available_balance = get_balance(
        &fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        false,
    );
    let alice_total_balance = get_balance(
        &fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        true,
    );

    assert_eq!(
        alice_available_balance.available_balance(),
        alice_total_balance.available_balance()
    );
}

#[tokio::test]
async fn erroneous_native_transfer_nofee_norefund_fixed() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = ConfigsOverride::default()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    // Transfer some token to Charlie.
    let (_txn_hash, _block, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        },
        None,
    )
    .await;
    assert!(exec_result_is_success(&exec_result));

    // Attempt to transfer more than Charlie has to Bob.
    let bob_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount + 100,
        &charlie_secret_key,
        PublicKey::from(&*bob_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: 1,
            additional_computation_factor: 0,
        },
        None,
    )
    .await;
    assert!(!exec_result_is_success(&exec_result)); // transaction should have failed.

    let expected_transfer_gas = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas; // since we set gas_price_tolerance to 1.

    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost,
        Gas::new(expected_transfer_gas),
        "failed_transfer_cost_fixed_price_no_fee_no_refund",
    );

    // Even though the transaction failed, a hold must still be in place for the transfer cost.
    let charlie_available_balance =
        get_balance(&fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_available_balance
            .available_balance()
            .expect("Expected Charlie to have a balance")
            .clone(),
        U512::from(transfer_amount) - expected_transfer_cost
    );
}

#[tokio::test]
async fn should_native_transfer_nofee_norefund_payment_limited() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&fixture, &alice_public_key, None, true)
        .available_balance()
        .expect("Expected Alice to have a balance.");

    const TRANSFER_PAYMENT: u64 = 100_000_000;

    // This transaction should be included since the tolerance is above the min gas price.
    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        TRANSFER_AMOUNT,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::PaymentLimited {
            payment_amount: TRANSFER_PAYMENT,
            gas_price_tolerance: MIN_GAS_PRICE + 1,
            standard_payment: true,
        },
        None,
    )
    .await;

    let expected_transfer_cost = TRANSFER_PAYMENT * MIN_GAS_PRICE as u64;

    assert!(exec_result_is_success(&exec_result)); // transaction should have succeeded.
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        Gas::new(TRANSFER_PAYMENT),
        "transfer_cost_payment_limited_price_no_fee_no_refund",
    );

    let alice_available_balance =
        get_balance(&fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance = get_balance(&fixture, &alice_public_key, Some(block_height), true);

    // since FeeHandling is set to NoFee, we expect that there's a hold on Alice's balance for the
    // cost of the transfer. The total balance of Alice now should be the initial balance - the
    // amount transferred to Charlie.
    let alice_expected_total_balance = alice_initial_balance - TRANSFER_AMOUNT;
    // The available balance is the initial balance - the amount transferred to Charlie - the hold
    // for the transfer cost.
    let alice_expected_available_balance = alice_expected_total_balance - expected_transfer_cost;

    assert_eq!(
        alice_total_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );

    let charlie_balance = get_balance(&fixture, &charlie_public_key, Some(block_height), false);
    assert_eq!(
        charlie_balance
            .available_balance()
            .expect("Expected Charlie to have a balance")
            .clone(),
        TRANSFER_AMOUNT.into()
    );

    // Check if the hold is released.
    let hold_release_block_height = block_height + 8; // Block time is 1s.
    fixture
        .run_until_block_height(hold_release_block_height, ONE_MIN)
        .await;

    let alice_available_balance = get_balance(
        &fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        false,
    );
    let alice_total_balance = get_balance(
        &fixture,
        &alice_public_key,
        Some(hold_release_block_height),
        true,
    );

    assert_eq!(
        alice_available_balance.available_balance(),
        alice_total_balance.available_balance()
    );
}

#[tokio::test]
async fn should_native_auction_with_nofee_norefund_payment_limited() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let alice_initial_balance = *get_balance(&fixture, &alice_public_key, None, true)
        .available_balance()
        .expect("Expected Alice to have a balance.");

    const BID_PAYMENT_AMOUNT: u64 = 2_500_000_000;

    let bid_amount = fixture.chainspec.core_config.minimum_bid_amount + 1;
    // This transaction should be included since the tolerance is above the min gas price.
    let (_txn_hash, block_height, exec_result) = send_add_bid(
        &mut fixture,
        bid_amount,
        &alice_secret_key,
        PricingMode::PaymentLimited {
            payment_amount: BID_PAYMENT_AMOUNT,
            gas_price_tolerance: MIN_GAS_PRICE + 1,
            standard_payment: true,
        },
    )
    .await;

    let expected_add_bid_consumed = fixture
        .chainspec
        .system_costs_config
        .auction_costs()
        .add_bid;
    let expected_add_bid_cost = expected_add_bid_consumed * MIN_GAS_PRICE as u64;

    assert!(exec_result_is_success(&exec_result)); // transaction should have succeeded.

    let transfers = exec_result.transfers();
    assert!(!transfers.is_empty(), "transfers should not be empty");
    assert_eq!(transfers.len(), 1, "transfers should have 1 entry");
    let transfer = transfers.first().expect("transfer entry should exist");
    let transfer_amount = transfer.amount();
    assert_eq!(
        transfer_amount,
        U512::from(bid_amount),
        "transfer amount should match the bid amount"
    );

    assert_exec_result_cost(
        exec_result,
        expected_add_bid_cost.into(),
        expected_add_bid_consumed.into(),
        "add_bid_with_classic_pricing_no_fee_no_refund",
    );

    let alice_available_balance =
        get_balance(&fixture, &alice_public_key, Some(block_height), false);
    let alice_total_balance = get_balance(&fixture, &alice_public_key, Some(block_height), true);

    // since FeeHandling is set to NoFee, we expect that there's a hold on Alice's balance for the
    // cost of the transfer. The total balance of Alice now should be the initial balance - the
    // amount transferred to Charlie.
    let alice_expected_total_balance = alice_initial_balance - bid_amount;
    // The available balance is the initial balance - the amount transferred to Charlie - the hold
    // for the transfer cost.
    let alice_expected_available_balance = alice_expected_total_balance - expected_add_bid_cost;

    assert_eq!(
        alice_total_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_total_balance
    );
    assert_eq!(
        alice_available_balance
            .available_balance()
            .expect("Expected Alice to have a balance")
            .clone(),
        alice_expected_available_balance
    );
}

#[tokio::test]
#[should_panic = "within 10 seconds"]
async fn should_reject_threshold_below_min_gas_price() {
    const TRANSFER_AMOUNT: u64 = 30_000_000_000;

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    // This transaction should NOT be included since the tolerance is below the min gas price.
    let (_, _, _) = transfer_to_account(
        &mut fixture,
        TRANSFER_AMOUNT,
        &alice_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::PaymentLimited {
            payment_amount: 1000,
            gas_price_tolerance: MIN_GAS_PRICE - 1,
            standard_payment: true,
        },
        None,
    )
    .await;
}

#[tokio::test]
async fn should_not_overcharge_native_operations_fixed() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(1, 2),
        })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&fixture, &bob_public_key, None, true)
        .total_balance()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&fixture, &alice_public_key, None, true)
        .total_balance()
        .expect("Expected Alice to have a balance.");

    let transfer_amount = fixture
        .chainspec
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let (_txn_hash, block_height, exec_result) = transfer_to_account(
        &mut fixture,
        transfer_amount,
        &bob_secret_key,
        PublicKey::from(&*charlie_secret_key),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
        None,
    )
    .await;

    assert!(exec_result_is_success(&exec_result)); // transaction should have succeeded.

    let expected_transfer_gas: u64 = fixture
        .chainspec
        .system_costs_config
        .mint_costs()
        .transfer
        .into();
    let expected_transfer_cost = expected_transfer_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
        "cost should equal consumed",
    );

    let bob_available_balance = *get_balance(&fixture, &bob_public_key, Some(block_height), false)
        .available_balance()
        .expect("Expected Bob to have a balance");
    let bob_total_balance = *get_balance(&fixture, &bob_public_key, Some(block_height), true)
        .total_balance()
        .expect("Expected Bob to have a balance");

    let alice_available_balance =
        *get_balance(&fixture, &alice_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Alice to have a balance");
    let alice_total_balance = *get_balance(&fixture, &alice_public_key, Some(block_height), true)
        .total_balance()
        .expect("Expected Alice to have a balance");

    // Bob shouldn't get a refund since there is no refund for native transfers.
    let bob_expected_total_balance = bob_initial_balance - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the full fee since there is no refund for native transfers.
    let alice_expected_total_balance = alice_initial_balance + expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    let charlie_balance = *get_balance(&fixture, &charlie_public_key, Some(block_height), false)
        .available_balance()
        .expect("Expected Charlie to have a balance");
    assert_eq!(charlie_balance.clone(), transfer_amount.into());

    assert_eq!(
        bob_available_balance.clone(),
        bob_expected_available_balance
    );

    assert_eq!(bob_total_balance.clone(), bob_expected_total_balance);

    assert_eq!(
        alice_available_balance.clone(),
        alice_expected_available_balance
    );

    assert_eq!(alice_total_balance.clone(), alice_expected_total_balance);
}

#[tokio::test]
async fn should_cancel_refund_for_erroneous_wasm() {
    // as a punitive measure, refunds are not issued for erroneous wasms even
    // if refunds are turned on.

    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]); // Node 0 is effectively guaranteed to be the proposer.

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut fixture = TestFixture::new(initial_stakes, Some(config)).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let bob_public_key = PublicKey::from(&*bob_secret_key);

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let bob_initial_balance = *get_balance(&fixture, &bob_public_key, None, true)
        .total_balance()
        .expect("Expected Bob to have a balance.");
    let alice_initial_balance = *get_balance(&fixture, &alice_public_key, None, true)
        .total_balance()
        .expect("Expected Alice to have a balance.");

    let (_txn_hash, block_height, exec_result) = send_wasm_transaction(
        &mut fixture,
        &bob_secret_key,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
    )
    .await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.

    let expected_transaction_gas: u64 = fixture
        .chainspec
        .get_max_gas_limit_by_category(LARGE_WASM_LANE_ID);
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0),
        "wasm_transaction_fees_are_refunded",
    );

    let bob_available_balance = *get_balance(&fixture, &bob_public_key, Some(block_height), false)
        .available_balance()
        .expect("Expected Bob to have a balance");
    let bob_total_balance = *get_balance(&fixture, &bob_public_key, Some(block_height), true)
        .total_balance()
        .expect("Expected Bob to have a balance");

    let alice_available_balance =
        *get_balance(&fixture, &alice_public_key, Some(block_height), false)
            .available_balance()
            .expect("Expected Alice to have a balance");
    let alice_total_balance = *get_balance(&fixture, &alice_public_key, Some(block_height), true)
        .total_balance()
        .expect("Expected Alice to have a balance");

    // Bob gets no refund because the wasm errored
    let bob_expected_total_balance = bob_initial_balance - expected_transaction_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get all the fee since it's set to pay to proposer
    // AND Bob didn't get a refund
    let alice_expected_total_balance = alice_initial_balance + expected_transaction_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_available_balance.clone(),
        bob_expected_available_balance
    );

    assert_eq!(bob_total_balance.clone(), bob_expected_total_balance);

    assert_eq!(
        alice_available_balance.clone(),
        alice_expected_available_balance
    );

    assert_eq!(alice_total_balance.clone(), alice_expected_total_balance);
}

#[tokio::test]
async fn should_refund_ratio_of_unconsumed_gas_fixed() {
    let refund_ratio = Ratio::new(1, 3);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = valid_wasm_txn(
        BOB_SECRET_KEY.clone(),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
    );
    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let gas_limit = txn
        .gas_limit(test.chainspec(), lane_id)
        .unwrap()
        .value()
        .as_u64();

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result));

    let expected_transaction_cost = gas_limit * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(DO_NOTHING_WASM_EXECUTION_GAS), /* Magic value, this is the amount of gas
                                                  * consumed by do_nothing.wasm */
        "wasm_transaction_fees_are_refunded",
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));

    // Bob should get 1/3 of the cost for the unspent gas. Since this transaction consumed 0
    // gas, the unspent gas is equal to the limit.
    let refund_amount: u64 = (refund_ratio
        * Ratio::from(
            expected_transaction_cost - DO_NOTHING_WASM_EXECUTION_GAS * MIN_GAS_PRICE as u64,
        ))
    .to_integer();

    let bob_expected_total_balance =
        bob_initial_balance.total.as_u64() - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance.total.as_u64() + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.as_u64(),
        bob_expected_available_balance
    );

    assert_eq!(
        bob_current_balance.total.as_u64(),
        bob_expected_total_balance
    );

    assert_eq!(
        alice_current_balance.available.as_u64(),
        alice_expected_available_balance
    );

    assert_eq!(
        alice_current_balance.total.as_u64(),
        alice_expected_total_balance
    );
}

async fn should_not_refund_erroneous_wasm_burn(txn_pricing_mode: PricingMode) {
    // if refund handling is set to burn, and an erroneous wasm is processed
    // ALL of the spent token is treated as the fee, thus there is no refund, and thus
    // nothing is burned.
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .get_max_gas_limit_by_category(LARGE_WASM_LANE_ID),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0),
        "wasm_transaction_refunds_are_burnt",
    );

    // Bobs transaction was invalid. He should get NO refund.
    // Since there is no refund - there will also be nothing burned.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt.
    let bob_expected_total_balance = bob_initial_balance.total - expected_transaction_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance = alice_initial_balance.total + expected_transaction_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

async fn should_burn_refunds(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, _gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 3);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = valid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let expected_transaction_gas = txn
        .gas_limit(test.chainspec(), lane_id)
        .unwrap()
        .value()
        .as_u64();
    let gas_cost = txn
        .gas_cost(test.chainspec(), lane_id, min_gas_price)
        .unwrap();
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result));
    assert_exec_result_cost(
        exec_result,
        gas_cost.value(),
        Gas::new(DO_NOTHING_WASM_EXECUTION_GAS),
        "wasm_transaction_refunds_are_burnt",
    );

    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    let refund_amount: U512 = (refund_ratio
        * Ratio::from(
            expected_transaction_cost - DO_NOTHING_WASM_EXECUTION_GAS * min_gas_price as u64,
        ))
    .to_integer()
    .into();

    // Bobs transaction was valid. He should get a refund.
    // 1/3 of the unspent gas should be burned
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt.
    let bob_expected_total_balance = bob_initial_balance.total - expected_transaction_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance.total + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_burn_refunds_fixed() {
    should_burn_refunds(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_burn_refunds_payment_limited() {
    should_burn_refunds(PricingMode::PaymentLimited {
        payment_amount: 2_500_000_001,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

#[tokio::test]
async fn should_not_refund_erroneous_wasm_burn_fixed() {
    should_not_refund_erroneous_wasm_burn(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_not_refund_erroneous_wasm_burn_payment_limited() {
    should_not_refund_erroneous_wasm_burn(PricingMode::PaymentLimited {
        payment_amount: 2_500_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn should_burn_refund_nofee(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, _gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = valid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let gas_limit = txn.gas_limit(test.chainspec(), lane_id).unwrap();
    let gas_cost = txn
        .gas_cost(test.chainspec(), lane_id, min_gas_price)
        .unwrap();
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    let consumed = exec_result.consumed().as_u64();
    let consumed_price = consumed * min_gas_price as u64;
    let expected_transaction_cost = gas_cost.value().as_u64();
    assert!(exec_result_is_success(&exec_result));
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(DO_NOTHING_WASM_EXECUTION_GAS), /* Magic value, this is the amount of gas
                                                  * consumed by do_nothing.wasm */
        "only_refunds_are_burnt_no_fee",
    );

    //TODO shouldn't this be (refund_ratio* Ratio::from((expected_transaction_cost -
    // consumed_price))?
    let refund_amount: U512 = (refund_ratio
        * Ratio::from((gas_limit.value().as_u64() * min_gas_price as u64) - consumed_price))
    .to_integer()
    .into();

    // We set it up so that the refunds are burnt so check this.
    let total_supply = test.get_total_supply(Some(block_height));
    assert_eq!(total_supply, initial_total_supply - refund_amount);

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt. A hold is put in place for the
    // transaction cost.
    let bob_balance_hold = U512::from(expected_transaction_cost) - refund_amount;
    let bob_expected_total_balance = bob_initial_balance.total - refund_amount;
    let bob_expected_available_balance = bob_current_balance.total - bob_balance_hold;

    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_burn_refund_nofee_fixed() {
    should_burn_refund_nofee(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_burn_refund_nofee_payment_limited() {
    should_burn_refund_nofee(PricingMode::PaymentLimited {
        payment_amount: 4_000_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn should_burn_fee_and_burn_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    // Fixed transaction pricing.
    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let expected_transaction_gas = gas_limit.unwrap_or(
        txn.gas_limit(test.chainspec(), lane_id)
            .unwrap()
            .value()
            .as_u64(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0),
        "fees_and_refunds_are_burnt_separately",
    );

    // Both refunds and fees should be burnt (even though they are burnt separately). Refund + fee
    // amounts to the txn cost so expect that the total supply is reduced by that amount.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - expected_transaction_cost
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // The refund and the fees are burnt. No holds should be in place.
    let bob_expected_total_balance = bob_initial_balance.total - expected_transaction_cost;
    let bob_expected_available_balance = bob_current_balance.total;

    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_burn_fee_and_burn_refund_fixed() {
    should_burn_fee_and_burn_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_burn_fee_and_burn_refund_payment_limited() {
    should_burn_fee_and_burn_refund(PricingMode::PaymentLimited {
        payment_amount: 2_500_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn should_burn_fee_erroneous_wasm(txn_pricing_mode: PricingMode) {
    // if erroneous wasm is processed, all the unconsumed amount goes to the fee
    // and is thus all of it is burned if FeeHandling == Burn
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);

    // Fixed transaction pricing.
    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let expected_transaction_gas = gas_limit.unwrap_or(
        txn.gas_limit(test.chainspec(), lane_id)
            .unwrap()
            .value()
            .as_u64(),
    );

    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0),
        "refunds_are_payed_and_fees_are_burnt",
    );

    // This transaction was erroneous, there should be no refund
    let refund_amount: U512 = U512::zero();

    // Only fees are burnt, so the refund_amount should still be in the total supply.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - expected_transaction_cost + refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob should get back the refund. The fees are burnt and no holds should be in place.
    let bob_expected_total_balance =
        bob_initial_balance.total - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_current_balance.total;

    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_burn_fee_erroneous_wasm_fixed() {
    should_burn_fee_erroneous_wasm(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_burn_fee_erroneous_wasm_payment_limited() {
    should_burn_fee_erroneous_wasm(PricingMode::PaymentLimited {
        payment_amount: 2_500_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn should_refund_unconsumed_and_gas_hold_fee(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, _gas_limit) = match_pricing_mode(&txn_pricing_mode);
    let refund_ratio = Ratio::new(1, 3);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = valid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);
    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let gas_limit = txn
        .gas_limit(test.chainspec(), lane_id)
        .unwrap()
        .value()
        .as_u64();

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result));

    let expected_transaction_cost = gas_limit * min_gas_price as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(DO_NOTHING_WASM_EXECUTION_GAS),
        "wasm_transaction_fees_are_refunded",
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));

    // Bob should get 1/3 of the cost for the unspent gas. Since this transaction consumed 0
    // gas, the unspent gas is equal to the limit.
    let refund_amount: u64 = (refund_ratio
        * Ratio::from(
            expected_transaction_cost - DO_NOTHING_WASM_EXECUTION_GAS * min_gas_price as u64,
        ))
    .to_integer();

    // Bob should get back the refund. The fees should be on hold, so Bob's total should be the
    // same as initial.
    let bob_expected_total_balance = bob_initial_balance.total;
    let bob_expected_available_balance =
        bob_current_balance.total - expected_transaction_cost + refund_amount;
    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_refund_unconsumed_and_gas_hold_fee_fixed() {
    should_refund_unconsumed_and_gas_hold_fee(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_refund_unconsumed_and_gas_hold_fee_payment_limited() {
    should_refund_unconsumed_and_gas_hold_fee(PricingMode::PaymentLimited {
        payment_amount: 2_500_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn should_gas_hold_fee_erroneous_wasm(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode);
    let meta_transaction = MetaTransaction::from_transaction(
        &txn,
        test.chainspec().core_config.pricing_handling,
        &test.chainspec().transaction_config,
    )
    .unwrap();
    // Fixed transaction pricing.
    let expected_consumed_gas = Gas::new(0); // expect that this transaction doesn't consume any gas since it has invalid wasm.
    let expected_transaction_gas = gas_limit.unwrap_or(
        meta_transaction
            .gas_limit(test.chainspec())
            .unwrap()
            .value()
            .as_u64(),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        expected_consumed_gas,
        "refunds_are_payed_and_fees_are_on_hold",
    );

    // Nothing is burnt so total supply should be the same.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob should get back the refund. The fees should be on hold, so Bob's total should be the
    // same as initial.
    let bob_expected_total_balance = bob_initial_balance.total;
    // There is no refund for bob because we don't pay refunds for transactions that errored during
    // execution
    let bob_expected_available_balance = bob_current_balance.total - expected_transaction_cost;

    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_gas_hold_fee_erroneous_wasm_fixed() {
    should_gas_hold_fee_erroneous_wasm(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn should_gas_hold_fee_erroneous_wasm_payment_limited() {
    should_gas_hold_fee_erroneous_wasm(PricingMode::PaymentLimited {
        payment_amount: 2_500_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

#[tokio::test]
async fn should_burn_fee_refund_unconsumed_custom_payment() {
    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    // This contract uses custom payment.
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("ee_601_regression.wasm");
    let module_bytes = Bytes::from(std::fs::read(contract_file).expect("cannot read module bytes"));

    let expected_transaction_gas = 2_500_000_000u64;
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: expected_transaction_gas,
            gas_price_tolerance: MIN_GAS_PRICE,
            standard_payment: false,
        })
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    match &exec_result {
        ExecutionResult::V2(exec_result_v2) => {
            assert_eq!(exec_result_v2.cost, expected_transaction_cost.into());
        }
        _ => {
            panic!("Unexpected exec result version.")
        }
    }

    let refund_amount = exec_result.refund().expect("should have refund");

    // Expect that the fees are burnt.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - expected_transaction_cost + refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob should get a refund. Since the contract doesn't set a custom purse for the refund, it
    // should get the refund in the main purse.
    let bob_expected_total_balance =
        bob_initial_balance.total - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_expected_total_balance; // No holds expected.

    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn should_allow_norefund_nofee_custom_payment() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    // This contract uses custom payment.
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("ee_601_regression.wasm");
    let module_bytes = Bytes::from(std::fs::read(contract_file).expect("cannot read module bytes"));

    let expected_transaction_gas = 1_000_000_000_000u64;
    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_pricing_mode(PricingMode::PaymentLimited {
            payment_amount: expected_transaction_gas,
            gas_price_tolerance: MIN_GAS_PRICE,
            standard_payment: false,
        })
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    match exec_result {
        ExecutionResult::V2(exec_result_v2) => {
            assert_eq!(exec_result_v2.cost, expected_transaction_cost.into());
        }
        _ => {
            panic!("Unexpected exec result version.")
        }
    }

    let payment_purse_balance = get_payment_purse_balance(&mut test.fixture, Some(block_height));
    assert_eq!(
        *payment_purse_balance
            .total_balance()
            .expect("should have total balance"),
        U512::zero(),
        "payment purse should have a 0 balance"
    );

    // we're not burning anything, so total supply should be the same
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply,
        "total supply should be the same before and after"
    );

    // updated balances
    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));

    // the proposer's balance should be the same because we are in no fee mode
    assert_eq!(
        alice_initial_balance, alice_current_balance,
        "the proposers balance should be unchanged as we are in no fee mode"
    );

    // the initiator should have a hold equal to the cost
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_initial_balance.total,
        "bob's total balance should be unchanged as we are in no fee mode"
    );

    assert_ne!(
        bob_current_balance.available.clone(),
        bob_initial_balance.total,
        "bob's available balance and total balance should not be the same"
    );

    let bob_expected_available_balance = bob_initial_balance.total - expected_transaction_cost;
    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance,
        "bob's available balance should reflect a hold for the cost"
    );
}

async fn transfer_fee_is_burnt_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::Burn);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = test
        .chainspec()
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let txn = transfer_txn(
        ALICE_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, _, charlie_initial_balance) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let charlie_initial_balance = charlie_initial_balance
        .expect("charlie should have balance")
        .total;

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    assert_eq!(exec_result.transfers().len(), 1, "{:?}", exec_result);
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
        "transfer_fee_is_burnt_no_refund",
    );

    // The fees should have been burnt so expect the total supply to have been
    // reduced by the fee that was burnt.
    let total_supply_after_txn = test.get_total_supply(Some(block_height));
    assert_ne!(
        total_supply_after_txn, initial_total_supply,
        "total supply should be lowered"
    );
    let diff = initial_total_supply - total_supply_after_txn;
    assert_eq!(
        diff,
        U512::from(expected_transfer_cost),
        "total supply should be lowered by expected transfer cost"
    );

    // Get the current balances after the transaction and check them.
    let (alice_current_balance, _, charlie_current_balance) = test.get_balances(Some(block_height));
    let alice_expected_total_balance =
        alice_initial_balance.total - transfer_amount - expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    let charlie_current_balance = charlie_current_balance
        .expect("charlie should have balance")
        .total;
    let charlies_expected_balance = charlie_initial_balance + transfer_amount;

    assert_eq!(
        charlie_current_balance, charlies_expected_balance,
        "expected balance does not match"
    );
    assert_eq!(
        alice_current_balance.available, alice_expected_available_balance,
        "alice available balance should match"
    );
    assert_eq!(alice_current_balance.total, alice_expected_total_balance);
}

#[tokio::test]
async fn transfer_fee_is_burnt_no_refund_fixed_pricing() {
    transfer_fee_is_burnt_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn transfer_fee_is_burnt_no_refund_payment_limited_pricing() {
    transfer_fee_is_burnt_no_refund(PricingMode::PaymentLimited {
        payment_amount: 100_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

// PTP == fee pay to proposer
async fn fee_ptp_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = test
        .chainspec()
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let charlie_initial_balance = charlie_initial_balance
        .expect("charlie should have balance")
        .total;

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    assert!(exec_result_is_success(&exec_result));
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
        "fee_is_payed_to_proposer_no_refund",
    );

    // Nothing should be burnt.
    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height)),
        "total supply should be unchanged"
    );

    let (alice_current_balance, bob_current_balance, charlie_current_balance) =
        test.get_balances(Some(block_height));

    let charlie_current_balance = charlie_current_balance
        .expect("charlie should still have balance")
        .total;
    let charlie_expected_balance = charlie_initial_balance.saturating_add(transfer_amount.into());
    assert_eq!(
        charlie_current_balance, charlie_expected_balance,
        "charlie's actual balance not expected total"
    );
    // since Alice was the proposer of the block, it should get back the transfer fee since
    // FeeHandling is set to PayToProposer.
    let bob_expected_total_balance =
        bob_initial_balance.total - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    let alice_expected_total_balance = alice_initial_balance.total + expected_transfer_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available,
        bob_expected_available_balance
    );
    assert_eq!(bob_current_balance.total, bob_expected_total_balance);
    assert_eq!(
        alice_current_balance.available,
        alice_expected_available_balance
    );
    assert_eq!(alice_current_balance.total, alice_expected_total_balance);
}

#[tokio::test]
async fn fee_ptp_norefund_fixed_pricing() {
    fee_ptp_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn fee_ptp_norefund_payment_limited() {
    fee_ptp_no_refund(PricingMode::PaymentLimited {
        payment_amount: 100_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn erroneous_wasm_transaction_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = invalid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode.clone());

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    let expected_transaction_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .get_max_gas_limit_by_category(LARGE_WASM_LANE_ID),
    );
    let expected_transaction_cost = expected_transaction_gas * min_gas_price as u64;

    assert!(!exec_result_is_success(&exec_result)); // transaction should not succeed because the wasm bytes are invalid.
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(0),
        format!(
            "erroneous_wasm_transaction_no_refund {:?}",
            txn_pricing_mode
        )
        .as_str(),
    );

    // Nothing is burnt so total supply should be the same.
    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height))
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob gets no refund, we don't pay refunds on erroneous wasm
    let bob_expected_total_balance = bob_initial_balance.total - expected_transaction_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get all the fee since it's set to pay to proposer and Bob got no refund
    let alice_expected_total_balance = alice_initial_balance.total + expected_transaction_cost;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

async fn wasm_transaction_ptp_fee_and_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let refund_ratio = Ratio::new(1, 3);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::Refund { refund_ratio })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let txn = valid_wasm_txn(BOB_SECRET_KEY.clone(), txn_pricing_mode.clone());
    let lane_id = calculate_transaction_lane_for_transaction(&txn, test.chainspec()).unwrap();
    let expected_transaction_gas = gas_limit.unwrap_or(
        txn.gas_limit(test.chainspec(), lane_id)
            .unwrap()
            .value()
            .as_u64(),
    );
    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let (alice_initial_balance, bob_initial_balance, _charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result));

    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        Gas::new(DO_NOTHING_WASM_EXECUTION_GAS),
        format!("wasm_transaction_ptp_fee_and_refund {:?}", txn_pricing_mode).as_str(),
    );

    // Nothing is burnt so total supply should be the same.
    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height))
    );

    // Bob should get back half of the cost for the unspent gas. Since this transaction consumed 0
    // gas, the unspent gas is equal to the limit.
    let refund_amount: U512 = (refund_ratio
        * Ratio::from(
            expected_transaction_cost - DO_NOTHING_WASM_EXECUTION_GAS * min_gas_price as u64,
        ))
    .to_integer()
    .into();

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    let bob_expected_total_balance =
        bob_initial_balance.total - expected_transaction_cost + refund_amount;
    let bob_expected_available_balance = bob_expected_total_balance;

    // Alice should get the non-refunded part of the fee since it's set to pay to proposer
    let alice_expected_total_balance =
        alice_initial_balance.total + expected_transaction_cost - refund_amount;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );
}

#[tokio::test]
async fn erroneous_wasm_transaction_norefund_fixed_pricing() {
    erroneous_wasm_transaction_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn wasm_transaction_refund_fixed_pricing() {
    wasm_transaction_ptp_fee_and_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn wasm_transaction_payment_limited_refund() {
    erroneous_wasm_transaction_no_refund(PricingMode::PaymentLimited {
        payment_amount: 2500000000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn fee_is_accumulated_and_distributed_no_refund(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let admins: BTreeSet<PublicKey> = vec![ALICE_PUBLIC_KEY.clone(), BOB_PUBLIC_KEY.clone()]
        .into_iter()
        .collect();

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::Accumulate)
        .with_administrators(admins);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = test
        .chainspec()
        .transaction_config
        .native_transfer_minimum_motes
        + 100;

    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let (alice_initial_balance, bob_initial_balance, charlie_initial_balance) =
        test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let acc_purse_initial_balance = *test
        .get_accumulate_purse_balance(None, false)
        .available_balance()
        .expect("Accumulate purse should have a balance.");

    let charlie_initial_balance = charlie_initial_balance
        .expect("Expected Charlie to have a balance")
        .total;

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result));
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
        "fee_is_accumulated_and_distributed_no_refund",
    );

    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height)),
        "total supply should remain unchanged"
    );

    let (alice_current_balance, bob_current_balance, charlie_current_balance) =
        test.get_balances(Some(block_height));

    let charlie_current_balance = charlie_current_balance
        .expect("Expected Charlie to have a balance")
        .total;
    let charlie_expected_balance =
        charlie_initial_balance.saturating_add(U512::from(transfer_amount));
    assert_eq!(
        charlie_current_balance, charlie_expected_balance,
        "charlie balance is not expected amount"
    );

    let bob_expected_total_balance =
        bob_initial_balance.total - transfer_amount - expected_transfer_cost;
    let bob_expected_available_balance = bob_expected_total_balance;

    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available,
        bob_expected_available_balance
    );
    assert_eq!(bob_current_balance.total, bob_expected_total_balance);
    assert_eq!(
        alice_current_balance.available,
        alice_expected_available_balance
    );
    assert_eq!(alice_current_balance.total, alice_expected_total_balance);

    let acc_purse_balance = *test
        .get_accumulate_purse_balance(Some(block_height), false)
        .available_balance()
        .expect("Accumulate purse should have a balance.");

    // The fees should be sent to the accumulation purse.
    assert_eq!(
        acc_purse_balance - acc_purse_initial_balance,
        expected_transfer_cost.into()
    );

    test.fixture
        .run_until_block_height(block_height + 10, ONE_MIN)
        .await;

    let accumulate_purse_balance = *test
        .get_accumulate_purse_balance(Some(block_height + 10), false)
        .available_balance()
        .expect("Accumulate purse should have a balance.");

    assert_eq!(accumulate_purse_balance, U512::from(0));
}

#[tokio::test]
async fn fee_is_accumulated_and_distributed_no_refund_fixed_pricing() {
    fee_is_accumulated_and_distributed_no_refund(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn fee_is_accumulated_and_distributed_no_refund_payment_limited_pricing() {
    fee_is_accumulated_and_distributed_no_refund(PricingMode::PaymentLimited {
        payment_amount: 100_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

fn transfer_txn<A: Into<U512>>(
    from: Arc<SecretKey>,
    to: &PublicKey,
    pricing_mode: PricingMode,
    amount: A,
) -> Transaction {
    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(amount, None, to.clone(), None)
            .unwrap()
            .with_initiator_addr(PublicKey::from(&*from))
            .with_pricing_mode(pricing_mode)
            .with_chain_name(CHAIN_NAME)
            .build()
            .unwrap(),
    );
    txn.sign(&from);
    txn
}

pub(crate) fn invalid_wasm_txn(
    initiator: Arc<SecretKey>,
    pricing_mode: PricingMode,
) -> Transaction {
    //These bytes are intentionally so large - this way they fall into "WASM_LARGE" category in the
    // local chainspec Alternatively we could change the chainspec to have a different limits
    // for the wasm categories, but that would require aligning all tests that use local
    // chainspec
    let module_bytes = Bytes::from(vec![1; 172_033]);
    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_pricing_mode(pricing_mode)
        .with_initiator_addr(PublicKey::from(&*initiator))
        .build()
        .unwrap(),
    );
    txn.sign(&initiator);
    txn
}

fn valid_wasm_txn(initiator: Arc<SecretKey>, pricing_mode: PricingMode) -> Transaction {
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("do_nothing.wasm");
    let module_bytes = Bytes::from(std::fs::read(contract_file).expect("cannot read module bytes"));
    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_pricing_mode(pricing_mode)
        .with_initiator_addr(PublicKey::from(&*initiator))
        .build()
        .unwrap(),
    );
    txn.sign(&initiator);
    txn
}

fn match_pricing_mode(txn_pricing_mode: &PricingMode) -> (PricingHandling, u8, Option<u64>) {
    match txn_pricing_mode {
        PricingMode::PaymentLimited {
            gas_price_tolerance,
            payment_amount,
            ..
        } => (
            PricingHandling::PaymentLimited,
            *gas_price_tolerance,
            Some(*payment_amount),
        ),
        PricingMode::Fixed {
            gas_price_tolerance,
            ..
        } => (PricingHandling::Fixed, *gas_price_tolerance, None),
        PricingMode::Prepaid { .. } => unimplemented!(),
    }
}

#[tokio::test]
async fn holds_should_be_added_and_cleared_fixed_pricing() {
    holds_should_be_added_and_cleared(PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    })
    .await;
}

#[tokio::test]
async fn holds_should_be_added_and_cleared_payment_limited_pricing() {
    holds_should_be_added_and_cleared(PricingMode::PaymentLimited {
        payment_amount: 100_000_000,
        gas_price_tolerance: MIN_GAS_PRICE,
        standard_payment: true,
    })
    .await;
}

async fn holds_should_be_added_and_cleared(txn_pricing_mode: PricingMode) {
    let (price_handling, min_gas_price, gas_limit) = match_pricing_mode(&txn_pricing_mode);

    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(price_handling)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );

    // transfer from bob to charlie
    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        txn_pricing_mode,
        transfer_amount,
    );

    let expected_transfer_gas: u64 = gas_limit.unwrap_or(
        test.chainspec()
            .system_costs_config
            .mint_costs()
            .transfer
            .into(),
    );
    let expected_transfer_cost = expected_transfer_gas * min_gas_price as u64;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (_, bob_initial_balance, charlie_initial_balance) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);
    let charlie_initial_balance = charlie_initial_balance.expect("should have balance").total;
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result); // transaction should have succeeded.
    assert_exec_result_cost(
        exec_result,
        expected_transfer_cost.into(),
        expected_transfer_gas.into(),
        "holds_should_be_added_and_cleared",
    );

    assert_eq!(
        initial_total_supply,
        test.get_total_supply(Some(block_height)),
        "total supply should remain unchanged"
    );

    // Get the current balances after the transaction and check them.
    let (_, bob_current_balance, charlie_current_balance) = test.get_balances(Some(block_height));

    let charlie_current_balance = charlie_current_balance.expect("should have balance").total;
    let charlie_expected_balance = charlie_initial_balance.saturating_add(transfer_amount);

    assert_eq!(
        charlie_current_balance, charlie_expected_balance,
        "charlie's balance should equal transfer amount"
    );
    assert_ne!(
        bob_current_balance.available, bob_current_balance.total,
        "total and available should NOT be equal at this point"
    );
    assert_eq!(
        bob_initial_balance.total,
        bob_current_balance.total + transfer_amount,
        "total balance should be original total balance - transferred amount"
    );
    assert_eq!(
        bob_initial_balance.total,
        bob_current_balance.available + expected_transfer_cost + transfer_amount,
        "diff from initial balance should equal available + cost + transfer_amount"
    );

    test.fixture
        .run_until_block_height(block_height + 5, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 5));
    assert_eq!(
        bob_balance.available, bob_balance.total,
        "total and available should be equal at this point"
    );
}

#[tokio::test]
async fn fee_holds_are_amortized() {
    let refund_ratio = Ratio::new(1, 2);
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::Burn { refund_ratio })
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Amortized)
        .with_balance_hold_interval(TimeDiff::from_seconds(10));

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    let txn = invalid_wasm_txn(
        BOB_SECRET_KEY.clone(),
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);
    let initial_total_supply = test.get_total_supply(None);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    // Fixed transaction pricing.
    let expected_transaction_gas: u64 = test
        .chainspec()
        .get_max_gas_limit_by_category(LARGE_WASM_LANE_ID);

    let expected_transaction_cost = expected_transaction_gas * MIN_GAS_PRICE as u64;
    // transaction should not succeed because the wasm bytes are invalid.
    // this transaction has invalid wasm, so the baseline will be used as consumed
    assert!(!exec_result_is_success(&exec_result));

    let expected_consumed = Gas::new(0);
    assert_exec_result_cost(
        exec_result,
        expected_transaction_cost.into(),
        expected_consumed,
        "fee_holds_are_amortized",
    );

    // This transaction consumed 0 gas, the unspent gas is equal to the limit, so we apply the
    // refund ratio to the full transaction cost.
    // error transactions no longer refund
    let refund_amount = U512::zero();

    // We set it up so that the refunds are burnt so check this.
    assert_eq!(
        test.get_total_supply(Some(block_height)),
        initial_total_supply - refund_amount
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // Bob doesn't get a refund. The refund is burnt. A hold is put in place for the
    // transaction cost.
    let bob_balance_hold = U512::from(expected_transaction_cost) - refund_amount;
    let bob_expected_total_balance = bob_initial_balance.total - refund_amount;
    let bob_expected_available_balance = bob_current_balance.total - bob_balance_hold;

    // Alice shouldn't get anything since we are operating with no fees
    let alice_expected_total_balance = alice_initial_balance.total;
    let alice_expected_available_balance = alice_expected_total_balance;

    assert_eq!(
        bob_current_balance.available.clone(),
        bob_expected_available_balance
    );
    assert_eq!(
        bob_current_balance.total.clone(),
        bob_expected_total_balance
    );
    assert_eq!(
        alice_current_balance.available.clone(),
        alice_expected_available_balance
    );
    assert_eq!(
        alice_current_balance.total.clone(),
        alice_expected_total_balance
    );

    let bob_prev_available_balance = bob_current_balance.available;
    test.fixture
        .run_until_block_height(block_height + 1, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 1));
    assert!(
        bob_prev_available_balance < bob_balance.available,
        "available should have increased since some part of the hold should have been amortized"
    );

    // Check to see if more holds have amortized.
    let bob_prev_available_balance = bob_current_balance.available;
    test.fixture
        .run_until_block_height(block_height + 3, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 3));
    assert!(
        bob_prev_available_balance < bob_balance.available,
        "available should have increased since some part of the hold should have been amortized"
    );

    // After 10s (10 blocks in this case) the holds should have been completely amortized
    test.fixture
        .run_until_block_height(block_height + 10, ONE_MIN)
        .await;
    let (_, bob_balance, _) = test.get_balances(Some(block_height + 10));
    assert_eq!(
        bob_balance.total, bob_balance.available,
        "available should have increased since some part of the hold should have been amortized"
    );
}

#[tokio::test]
async fn sufficient_balance_is_available_after_amortization() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Amortized)
        .with_balance_hold_interval(TimeDiff::from_seconds(10));

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_cost: U512 =
        U512::from(test.chainspec().system_costs_config.mint_costs().transfer) * MIN_GAS_PRICE;
    let min_transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );
    let half_transfer_cost =
        (Ratio::new(U512::from(1), U512::from(2)) * transfer_cost).to_integer();

    // Fund Charlie with some token.
    let transfer_amount = min_transfer_amount * 2 + transfer_cost + half_transfer_cost;
    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let charlie_initial_balance = test.get_balances(None).2.expect("should have balance");

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));
    let charlie_expected_balance = charlie_initial_balance.total + transfer_amount;
    let charlie_current_balance = test.get_balances(Some(block_height)).2.unwrap();

    assert_eq!(
        charlie_current_balance.available.clone(),
        charlie_expected_balance,
        "balance does not match expected"
    );

    // Now Charlie has balance to do 2 transfers of the minimum amount but can't pay for both as the
    // same time. Let's say the min transfer amount is 2_500_000_000 and the cost of a transfer
    // is 50_000. Charlie now has 5_000_075_000 as set up above. He can transfer 2_500_000_000
    // which will put a hold of 50_000. His available balance would be 2_500_025_000.
    // He can't issue a new transfer of 2_500_000_000 right away because he doesn't have enough
    // balance to pay for the transfer. He'll need to wait until at least half of the holds
    // amortize. In this case he needs to wait half of the amortization time for 25_000 to
    // become available to him. After this period, he will have 2_500_050_000 available which
    // will allow him to do another transfer.
    let txn = transfer_txn(
        CHARLIE_SECRET_KEY.clone(),
        &BOB_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
        min_transfer_amount,
    );
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    let charlie_updated_balance = test
        .get_balances(Some(block_height))
        .2
        .expect("should have balance");
    /* one `min_transfer_amount` * should have gone to Bob. */
    let expected =
        charlie_initial_balance.total + min_transfer_amount + transfer_cost + half_transfer_cost;
    assert_eq!(
        charlie_updated_balance.total.clone(),
        expected,
        "unexpected balance"
    );

    // transfer cost should be held.
    let expected_available =
        charlie_initial_balance.total + min_transfer_amount + half_transfer_cost;
    assert_eq!(
        charlie_updated_balance.available.clone(),
        expected_available,
        "charlie updated available should represent held cost"
    );

    // Let's wait for about 5 sec (5 blocks in this case) which should provide enough time for
    // half of the holds to get amortized.
    test.fixture
        .run_until_block_height(block_height + 5, ONE_MIN)
        .await;
    let charlie_post5_balance = test.get_balances(Some(block_height + 5)).2.unwrap();
    /* right now he should have enough to make a transfer. */
    assert!(charlie_post5_balance.available >= min_transfer_amount + transfer_cost,);
    /* some of the holds should still be in place. */
    assert!(charlie_post5_balance.available < charlie_post5_balance.total,);

    // Send another transfer to Bob for `min_transfer_amount`.
    let txn = transfer_txn(
        CHARLIE_SECRET_KEY.clone(),
        &BOB_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
        min_transfer_amount,
    );

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;

    assert!(exec_result_is_success(&exec_result)); // We expect this transfer to succeed since Charlie has enough balance.
    let charlie_final_balance = test.get_balances(Some(block_height)).2.unwrap();

    let expected_total = charlie_post5_balance.total - min_transfer_amount;
    assert_eq!(
        charlie_final_balance.total, expected_total,
        "total should match prior amount minus transferred amount"
    );

    assert!(
        charlie_final_balance.available < charlie_final_balance.total,
        "some of the holds should still be in place"
    );

    test.fixture
        .run_until_block_height(block_height + 15, ONE_MIN)
        .await;
    let charlie_post15_balance = test.get_balances(Some(block_height + 15)).2.unwrap();

    assert_eq!(
        charlie_post15_balance.available, charlie_post15_balance.total,
        "all holds should have amortized back"
    );
}

#[tokio::test]
async fn validator_credit_is_written_and_cleared_after_auction() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_cost: U512 =
        U512::from(test.chainspec().system_costs_config.mint_costs().transfer) * MIN_GAS_PRICE;
    let min_transfer_amount = U512::from(
        test.chainspec()
            .transaction_config
            .native_transfer_minimum_motes,
    );
    let half_transfer_cost =
        (Ratio::new(U512::from(1), U512::from(2)) * transfer_cost).to_integer();

    // Fund Charlie with some token.
    let transfer_amount = min_transfer_amount * 2 + transfer_cost + half_transfer_cost;
    let txn = transfer_txn(
        BOB_SECRET_KEY.clone(),
        &CHARLIE_PUBLIC_KEY,
        PricingMode::Fixed {
            gas_price_tolerance: MIN_GAS_PRICE,
            additional_computation_factor: 0,
        },
        transfer_amount,
    );

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;
    let charlie_initial_balance = test.get_balances(None).2.unwrap();
    assert_eq!(
        charlie_initial_balance.available.clone(),
        charlie_initial_balance.total.clone(),
        "there should be no holds"
    );

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));
    let charlie_current_balance = test.get_balances(Some(block_height)).2.unwrap();

    assert_eq!(
        charlie_current_balance.available, charlie_current_balance.total,
        "there should be no holds"
    );
    assert_eq!(
        charlie_initial_balance.total + transfer_amount,
        charlie_current_balance.total,
        "current balance should include received amount"
    );

    let bids =
        get_bids(&mut test.fixture, Some(block_height)).expect("Expected to get some bid records.");

    let _ = bids
        .into_iter()
        .find(|bid_kind| match bid_kind {
            BidKind::Credit(credit) => {
                credit.amount() == transfer_cost
                    && credit.validator_public_key() == &*ALICE_PUBLIC_KEY // Alice is the proposer.
            }
            _ => false,
        })
        .expect("Expected to find the credit for the consumed transfer cost in the bid records.");

    test.fixture
        .run_until_consensus_in_era(
            ERA_ONE.saturating_add(test.chainspec().core_config.auction_delay),
            ONE_MIN,
        )
        .await;

    // Check that the credits were cleared after the auction.
    let bids = get_bids(&mut test.fixture, None).expect("Expected to get some bid records.");
    assert!(!bids
        .into_iter()
        .any(|bid| matches!(bid, BidKind::Credit(_))));
}

#[tokio::test]
async fn add_and_withdraw_bid_transaction() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let bid_amount = test.chainspec().core_config.minimum_bid_amount + 10;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_add_bid(
            PublicKey::from(&**BOB_SECRET_KEY),
            0,
            bid_amount,
            None,
            None,
            None,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (_, _bob_initial_balance, _) = test.get_balances(None);
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    test.fixture
        .run_until_consensus_in_era(ERA_TWO, ONE_MIN)
        .await;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_withdraw_bid(PublicKey::from(&**BOB_SECRET_KEY), bid_amount)
            .unwrap()
            .with_chain_name(CHAIN_NAME)
            .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
            .build()
            .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));
}

#[tokio::test]
async fn delegate_and_undelegate_bid_transaction() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let delegate_amount = U512::from(500_000_000_000u64);

    let mut txn = Transaction::from(
        TransactionV1Builder::new_delegate(
            PublicKey::from(&**BOB_SECRET_KEY),
            PublicKey::from(&**ALICE_SECRET_KEY),
            delegate_amount,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let mut txn = Transaction::from(
        TransactionV1Builder::new_undelegate(
            PublicKey::from(&**BOB_SECRET_KEY),
            PublicKey::from(&**ALICE_SECRET_KEY),
            delegate_amount,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result));
}

#[tokio::test]
async fn insufficient_funds_transfer_from_account() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = U512::max_value();

    let txn_v1 =
        TransactionV1Builder::new_transfer(transfer_amount, None, ALICE_PUBLIC_KEY.clone(), None)
            .unwrap()
            .with_chain_name(CHAIN_NAME)
            .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
            .build()
            .unwrap();

    let mut txn = Transaction::from(txn_v1);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };
    let expected_cost: U512 = U512::from(DEFAULT_TRANSFER_COST);

    assert_eq!(result.error_message.as_deref(), Some("Insufficient funds"));
    assert_eq!(result.cost, expected_cost);
}

#[tokio::test]
async fn insufficient_funds_add_bid() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (_, bob_initial_balance, _) = test.get_balances(None);
    let bid_amount = bob_initial_balance.total;

    let txn =
        TransactionV1Builder::new_add_bid(BOB_PUBLIC_KEY.clone(), 0, bid_amount, None, None, None)
            .unwrap()
            .with_chain_name(CHAIN_NAME)
            .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
            .build()
            .unwrap();
    let price = txn.payment_amount().expect("must get payment amount");
    let mut txn = Transaction::from(txn);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };
    let bid_cost: U512 = U512::from(price) * MIN_GAS_PRICE;

    assert_eq!(
        result.error_message.as_deref(),
        Some("ApiError::AuctionError(TransferToBidPurse) [64516]")
    );
    assert_eq!(result.cost, bid_cost);
}

#[tokio::test]
async fn insufficient_funds_transfer_from_purse() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let purse_name = "test_purse";

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // first we set up a purse for Bob
    let purse_create_contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("transfer_main_purse_to_new_purse.wasm");
    let module_bytes =
        Bytes::from(std::fs::read(purse_create_contract).expect("cannot read module bytes"));

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_runtime_args(runtime_args! { "destination" => purse_name, "amount" => U512::zero() })
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);

    let state_root_hash = *test.fixture.highest_complete_block().state_root_hash();
    let entity_addr = get_entity_addr_from_account_hash(
        &mut test.fixture,
        state_root_hash,
        BOB_PUBLIC_KEY.to_account_hash(),
    );
    let key = get_entity_named_key(&mut test.fixture, state_root_hash, entity_addr, purse_name)
        .expect("expected a key");
    let uref = *key.as_uref().expect("Expected a URef");

    // now we try to transfer from the purse we just created
    let transfer_amount = U512::max_value();
    let txn = TransactionV1Builder::new_transfer(
        transfer_amount,
        Some(uref),
        ALICE_PUBLIC_KEY.clone(),
        None,
    )
    .unwrap()
    .with_chain_name(CHAIN_NAME)
    .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
    .build()
    .unwrap();

    let mut txn = Transaction::from(txn);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };
    let expected_cost: U512 = U512::from(DEFAULT_TRANSFER_COST);

    assert_eq!(result.error_message.as_deref(), Some("Insufficient funds"));
    assert_eq!(result.cost, expected_cost);
}

#[tokio::test]
async fn insufficient_funds_when_caller_lacks_minimum_balance() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (_, bob_initial_balance, _) = test.get_balances(None);
    let transfer_amount = bob_initial_balance.total - U512::one();
    let txn =
        TransactionV1Builder::new_transfer(transfer_amount, None, ALICE_PUBLIC_KEY.clone(), None)
            .unwrap()
            .with_chain_name(CHAIN_NAME)
            .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
            .build()
            .unwrap();

    let mut txn = Transaction::from(txn);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };
    let expected_cost: U512 = U512::from(DEFAULT_TRANSFER_COST);

    assert_eq!(result.error_message.as_deref(), Some("Insufficient funds"));
    assert_eq!(result.cost, expected_cost);
}

#[tokio::test]
async fn charge_when_session_code_succeeds() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("transfer_purse_to_account.wasm");
    let module_bytes = Bytes::from(std::fs::read(contract).expect("cannot read module bytes"));

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);

    let transferred_amount = 1;
    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_runtime_args(runtime_args! {
            ARG_TARGET => CHARLIE_PUBLIC_KEY.to_account_hash(),
            ARG_AMOUNT => U512::from(transferred_amount)
        })
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .with_pricing_mode(PricingMode::Fixed {
            gas_price_tolerance: 5,
            additional_computation_factor: 2, /*Makes the transaction
                                               * "Large" despite the fact that the actual
                                               * WASM bytes categorize it as "Small" */
        })
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // alice should get the fee since she is the proposer.
    let fee = alice_current_balance.total - alice_initial_balance.total;

    assert!(
        fee > U512::zero(),
        "fee is {}, expected to be greater than 0",
        fee
    );
    assert_eq!(
        bob_current_balance.total,
        bob_initial_balance.total - transferred_amount - fee,
        "bob should pay the fee"
    );
}

#[tokio::test]
async fn charge_when_session_code_fails_with_user_error() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let revert_contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("revert.wasm");
    let module_bytes =
        Bytes::from(std::fs::read(revert_contract).expect("cannot read module bytes"));

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(
        matches!(
            &exec_result,
            ExecutionResult::V2(res) if res.error_message.as_deref() == Some("User error: 100")
        ),
        "{:?}",
        exec_result.error_message()
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // alice should get the fee since she is the proposer.
    let fee = alice_current_balance.total - alice_initial_balance.total;

    assert!(
        fee > U512::zero(),
        "fee is {}, expected to be greater than 0",
        fee
    );
    let init = bob_initial_balance.total;
    let curr = bob_current_balance.total;
    let actual = curr;
    let expected = init - fee;
    assert_eq!(actual, expected, "init {} curr {} fee {}", init, curr, fee,);
}

#[tokio::test]
async fn charge_when_session_code_runs_out_of_gas() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let revert_contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("endless_loop.wasm");
    let module_bytes =
        Bytes::from(std::fs::read(revert_contract).expect("cannot read module bytes"));

    let (alice_initial_balance, bob_initial_balance, _) = test.get_balances(None);

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(
        matches!(
            &exec_result,
            ExecutionResult::V2(res) if res.error_message.as_deref() == Some("Out of gas error")
        ),
        "{:?}",
        exec_result
    );

    let (alice_current_balance, bob_current_balance, _) = test.get_balances(Some(block_height));
    // alice should get the fee since she is the proposer.
    let fee = alice_current_balance.total - alice_initial_balance.total;

    assert!(
        fee > U512::zero(),
        "fee is {}, expected to be greater than 0",
        fee
    );
    assert_eq!(
        bob_current_balance.total,
        bob_initial_balance.total - fee,
        "bob should pay the fee"
    );
}

#[tokio::test]
async fn successful_purse_to_purse_transfer() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let purse_name = "test_purse";

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, _, _) = test.get_balances(None);

    // first we set up a purse for Bob
    let purse_create_contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("transfer_main_purse_to_new_purse.wasm");
    let module_bytes =
        Bytes::from(std::fs::read(purse_create_contract).expect("cannot read module bytes"));

    let baseline_motes = test
        .fixture
        .chainspec
        .core_config
        .baseline_motes_amount_u512();

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_runtime_args(
            runtime_args! { "destination" => purse_name, "amount" => baseline_motes + U512::one() },
        )
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);

    let state_root_hash = *test.fixture.highest_complete_block().state_root_hash();
    let bob_addr = get_entity_addr_from_account_hash(
        &mut test.fixture,
        state_root_hash,
        BOB_PUBLIC_KEY.to_account_hash(),
    );
    let bob_purse_key =
        get_entity_named_key(&mut test.fixture, state_root_hash, bob_addr, purse_name)
            .expect("expected a key");
    let bob_purse = *bob_purse_key.as_uref().expect("Expected a URef");

    let alice_addr = get_entity_addr_from_account_hash(
        &mut test.fixture,
        state_root_hash,
        ALICE_PUBLIC_KEY.to_account_hash(),
    );
    let alice = get_entity(&mut test.fixture, state_root_hash, alice_addr);

    // now we try to transfer from the purse we just created
    let transfer_amount = 1;
    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(
            transfer_amount,
            Some(bob_purse),
            alice.main_purse(),
            None,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);

    let (alice_current_balance, _, _) = test.get_balances(Some(block_height));
    assert_eq!(
        alice_current_balance.total,
        alice_initial_balance.total + transfer_amount,
    );
}

#[tokio::test]
async fn successful_purse_to_account_transfer() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let purse_name = "test_purse";

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let (alice_initial_balance, _, _) = test.get_balances(None);

    // first we set up a purse for Bob
    let purse_create_contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("transfer_main_purse_to_new_purse.wasm");
    let module_bytes =
        Bytes::from(std::fs::read(purse_create_contract).expect("cannot read module bytes"));

    let baseline_motes = test
        .fixture
        .chainspec
        .core_config
        .baseline_motes_amount_u512();
    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_runtime_args(
            runtime_args! { "destination" => purse_name, "amount" => baseline_motes + U512::one() },
        )
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);

    let state_root_hash = *test.fixture.highest_complete_block().state_root_hash();
    let bob_addr = get_entity_addr_from_account_hash(
        &mut test.fixture,
        state_root_hash,
        BOB_PUBLIC_KEY.to_account_hash(),
    );
    let bob_purse_key =
        get_entity_named_key(&mut test.fixture, state_root_hash, bob_addr, purse_name)
            .expect("expected a key");
    let bob_purse = *bob_purse_key.as_uref().expect("Expected a URef");

    // now we try to transfer from the purse we just created
    let transfer_amount = 1;
    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(
            transfer_amount,
            Some(bob_purse),
            ALICE_PUBLIC_KEY.clone(),
            None,
        )
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);

    let (alice_current_balance, _, _) = test.get_balances(Some(block_height));
    assert_eq!(
        alice_current_balance.total,
        alice_initial_balance.total + transfer_amount,
    );
}

async fn bob_transfers_to_charlie_via_native_transfer_deploy(
    configs_override: ConfigsOverride,
    with_source: bool,
) -> ExecutionResult {
    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(configs_override),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let state_root_hash = *test.fixture.highest_complete_block().state_root_hash();
    let entity = get_entity_by_account_hash(
        &mut test.fixture,
        state_root_hash,
        BOB_PUBLIC_KEY.to_account_hash(),
    );

    let source = if with_source {
        Some(entity.main_purse())
    } else {
        None
    };

    let mut txn: Transaction = Deploy::native_transfer(
        CHAIN_NAME.to_string(),
        source,
        BOB_PUBLIC_KEY.clone(),
        CHARLIE_PUBLIC_KEY.clone(),
        None,
        Timestamp::now(),
        TimeDiff::from_seconds(600),
        10,
    )
    .into();
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    exec_result
}

#[tokio::test]
async fn should_transfer_with_source_purse_deploy_fixed_norefund_nofee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);
    let exec_result = bob_transfers_to_charlie_via_native_transfer_deploy(config, true).await;

    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    assert_eq!(
        exec_result.transfers().len(),
        1,
        "native transfer should have exactly 1 transfer"
    );
}

#[tokio::test]
async fn should_transfer_with_source_purse_deploy_payment_limited_refund_fee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(99, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);
    let exec_result = bob_transfers_to_charlie_via_native_transfer_deploy(config, true).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    assert_eq!(
        exec_result.transfers().len(),
        1,
        "native transfer should have exactly 1 transfer"
    );
    assert_eq!(
        exec_result.refund(),
        Some(U512::zero()),
        "cost should equal consumed thus no refund"
    );
}

#[tokio::test]
async fn should_charge_for_insufficient_funds_deploy_payment_limited_refund_fee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(75, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;
    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let charlie_balance = test
        .get_balances(None)
        .2
        .expect("should have charlie balance")
        .available;

    assert_eq!(
        charlie_balance,
        U512::from(u32::MAX - 1),
        "charlie balance should be u32::MAX - 1"
    );
    let payment_amount = charlie_balance.saturating_add(U512::from(1)).as_u64();

    let txn = valid_wasm_txn(
        CHARLIE_SECRET_KEY.clone(),
        PricingMode::PaymentLimited {
            payment_amount,
            gas_price_tolerance: 1,
            standard_payment: true,
        },
    );
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };

    assert!(!result.effects.is_empty(), "should have effects");
    let expected_cost: U512 = charlie_balance;

    assert_eq!(result.error_message.as_deref(), Some("Insufficient funds"));
    assert_eq!(result.cost, expected_cost);
}

#[tokio::test]
async fn should_charge_for_marginal_insufficient_funds_deploy_payment_limited_refund_fee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(75, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer);

    let base_amount = 100_000_000_000_000_000u64;
    let charlie_base_amount = 10_000_000_000u64;
    let mut test = {
        let alice_public_key = PublicKey::from(&*ALICE_SECRET_KEY.clone());
        let bob_public_key = PublicKey::from(&*BOB_SECRET_KEY.clone());
        let charlie_public_key = PublicKey::from(&*CHARLIE_SECRET_KEY.clone());

        let stakes = {
            let mut ret = BTreeMap::new();
            ret.insert(
                alice_public_key.clone(),
                (U512::from(base_amount), U512::from(u128::MAX)),
            );
            ret.insert(
                bob_public_key.clone(),
                (U512::from(base_amount), U512::from(1)),
            );

            ret.insert(
                charlie_public_key,
                (U512::from(charlie_base_amount), U512::from(1)),
            );
            ret
        };

        SingleTransactionTestCase::new_with_stakes(
            ALICE_SECRET_KEY.clone(),
            BOB_SECRET_KEY.clone(),
            CHARLIE_SECRET_KEY.clone(),
            Some(config),
            stakes,
        )
        .await
    };

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let charlie_balance = test
        .get_balances(None)
        .2
        .expect("should have charlie balance")
        .available;

    assert_eq!(
        charlie_balance,
        U512::from(charlie_base_amount),
        "charlie balance should be charlie_base_amount"
    );
    // make payment 1 more than charlie has
    let payment_amount = charlie_balance.saturating_add(U512::from(1)).as_u64();

    let txn = valid_wasm_txn(
        CHARLIE_SECRET_KEY.clone(),
        PricingMode::PaymentLimited {
            payment_amount,
            gas_price_tolerance: 1,
            standard_payment: true,
        },
    );
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };

    assert!(!result.effects.is_empty(), "should have effects");
    let expected_cost: U512 = charlie_balance;

    assert_eq!(result.error_message.as_deref(), Some("Insufficient funds"));
    assert_eq!(result.cost, expected_cost);
}

async fn make_new_account_and_exec_wasm(
    config: ConfigsOverride,
    initial_balance: u64,
    wasm_payment_amount: u64,
) -> Result<ExecutionResult, String> {
    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // fund a new account
    let dan_secret_key =
        Arc::new(SecretKey::ed25519_from_bytes([0xDD; SecretKey::ED25519_LENGTH]).unwrap());
    let dan_public_key = PublicKey::from(&*dan_secret_key.clone());
    let _dan_account_hash = dan_public_key.to_account_hash();

    let transfer_payment_amount = 100_000_000u64;
    let txn = transfer_txn(
        ALICE_SECRET_KEY.clone(),
        &dan_public_key,
        PricingMode::PaymentLimited {
            payment_amount: transfer_payment_amount,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
        initial_balance,
    );

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        return Err(format!(
            "Expected ExecutionResult::V2 but got {:?}",
            exec_result
        ));
    };

    assert!(!result.effects.is_empty(), "should have effects");
    assert!(
        result.error_message.is_none(),
        "transfer to create new account should not have error msg"
    );

    let txn = valid_wasm_txn(
        dan_secret_key.clone(),
        PricingMode::PaymentLimited {
            payment_amount: wasm_payment_amount,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
    );
    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;

    Ok(exec_result)
}

#[tokio::test]
async fn should_charge_new_account_insufficient_funds_deploy_payment_limited_refund_fee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(75, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer);

    let dan_base_amount = 10_000_000_000u64;
    // pay more than available with the new account
    let wasm_payment_amount = dan_base_amount.saturating_add(100);
    match make_new_account_and_exec_wasm(config, dan_base_amount, wasm_payment_amount).await {
        Ok(exec_result) => {
            let ExecutionResult::V2(exec_result) = exec_result else {
                panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
            };
            assert!(!exec_result.effects.is_empty(), "should have effects");
            let expected_cost: U512 = dan_base_amount.into();

            assert_eq!(
                exec_result.error_message.as_deref(),
                Some("Insufficient funds")
            );
            assert_eq!(
                exec_result.cost, expected_cost,
                "cost should be expected val"
            );
            assert_eq!(exec_result.refund, U512::zero(), "refund should be 0");
        }
        Err(err_str) => {
            panic!("{}", err_str)
        }
    }
}

#[tokio::test]
async fn should_charge_new_account_insufficient_funds_deploy_payment_limited_refund_fee_price_2() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(75, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_min_gas_price(2)
        .with_max_gas_price(3);

    let dan_base_amount = 10_000_000_000u64;
    // pay more than available with the new account
    let wasm_payment_amount = dan_base_amount.saturating_add(100);
    match make_new_account_and_exec_wasm(config, dan_base_amount, wasm_payment_amount).await {
        Ok(exec_result) => {
            let ExecutionResult::V2(exec_result) = exec_result else {
                panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
            };
            assert!(!exec_result.effects.is_empty(), "should have effects");
            let expected_cost: U512 = dan_base_amount.into();

            assert_eq!(
                exec_result.error_message.as_deref(),
                Some("Insufficient funds")
            );
            assert_eq!(
                exec_result.cost, expected_cost,
                "cost should be expected val"
            );
            assert_eq!(exec_result.refund, U512::zero(), "refund should be 0");
        }
        Err(err_str) => {
            panic!("{}", err_str)
        }
    }
}

#[tokio::test]
async fn should_transfer_with_main_purse_deploy_fixed_norefund_nofee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);
    let exec_result = bob_transfers_to_charlie_via_native_transfer_deploy(config, false).await;

    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    assert_eq!(
        exec_result.transfers().len(),
        1,
        "native transfer should have exactly 1 transfer"
    );
}

#[tokio::test]
async fn should_transfer_with_main_purse_deploy_payment_limited_refund_fee() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(99, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);
    let exec_result = bob_transfers_to_charlie_via_native_transfer_deploy(config, false).await;
    assert!(exec_result_is_success(&exec_result), "{:?}", exec_result);
    assert_eq!(
        exec_result.transfers().len(),
        1,
        "native transfer should have exactly 1 transfer"
    );
    assert_eq!(
        exec_result.refund(),
        Some(U512::zero()),
        "cost should equal consumed thus no refund"
    );
}

#[tokio::test]
async fn out_of_gas_txn_does_not_produce_effects() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::PayToProposer);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // This WASM creates named key called "new_key". Then it would loop endlessly trying to write a
    // value to storage. Eventually it will run out of gas and it should exit causing a revert.
    let revert_contract = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("endless_loop_with_effects.wasm");
    let module_bytes =
        Bytes::from(std::fs::read(revert_contract).expect("cannot read module bytes"));

    let mut txn = Transaction::from(
        TransactionV1Builder::new_session(
            false,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
        )
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(BOB_PUBLIC_KEY.clone())
        .build()
        .unwrap(),
    );
    txn.sign(&BOB_SECRET_KEY);
    let (_txn_hash, block_height, exec_result) = test.send_transaction(txn).await;
    assert!(
        matches!(
            &exec_result,
            ExecutionResult::V2(res) if res.error_message.as_deref() == Some("Out of gas error")
        ),
        "{:?}",
        exec_result
    );

    let state_root_hash = *test
        .fixture
        .get_block_by_height(block_height)
        .state_root_hash();
    let bob_addr = get_entity_addr_from_account_hash(
        &mut test.fixture,
        state_root_hash,
        BOB_PUBLIC_KEY.to_account_hash(),
    );

    // Named key should not exist since the execution was reverted because it was out of gas.
    assert!(
        get_entity_named_key(&mut test.fixture, state_root_hash, bob_addr, "new_key").is_none()
    );
}

#[tokio::test]
async fn gas_holds_accumulate_for_multiple_transactions_in_the_same_block() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_min_gas_price(MIN_GAS_PRICE)
        .with_max_gas_price(MIN_GAS_PRICE)
        .with_pricing_handling(PricingHandling::Fixed)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee)
        .with_balance_hold_interval(TimeDiff::from_seconds(5));

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    const TRANSFER_AMOUNT: u64 = 30_000_000_000;

    let chain_name = test.fixture.chainspec.network_config.name.clone();
    let txn_pricing_mode = PricingMode::Fixed {
        gas_price_tolerance: MIN_GAS_PRICE,
        additional_computation_factor: 0,
    };
    let expected_transfer_gas = test.chainspec().system_costs_config.mint_costs().transfer;
    let expected_transfer_cost: U512 = U512::from(expected_transfer_gas) * MIN_GAS_PRICE;

    let mut txn_1 = Transaction::from(
        TransactionV1Builder::new_transfer(TRANSFER_AMOUNT, None, CHARLIE_PUBLIC_KEY.clone(), None)
            .unwrap()
            .with_initiator_addr(ALICE_PUBLIC_KEY.clone())
            .with_pricing_mode(txn_pricing_mode.clone())
            .with_chain_name(chain_name.clone())
            .build()
            .unwrap(),
    );
    txn_1.sign(&ALICE_SECRET_KEY);
    let txn_1_hash = txn_1.hash();

    let mut txn_2 = Transaction::from(
        TransactionV1Builder::new_transfer(
            2 * TRANSFER_AMOUNT,
            None,
            CHARLIE_PUBLIC_KEY.clone(),
            None,
        )
        .unwrap()
        .with_initiator_addr(ALICE_PUBLIC_KEY.clone())
        .with_pricing_mode(txn_pricing_mode.clone())
        .with_chain_name(chain_name.clone())
        .build()
        .unwrap(),
    );
    txn_2.sign(&ALICE_SECRET_KEY);
    let txn_2_hash = txn_2.hash();

    let mut txn_3 = Transaction::from(
        TransactionV1Builder::new_transfer(
            3 * TRANSFER_AMOUNT,
            None,
            CHARLIE_PUBLIC_KEY.clone(),
            None,
        )
        .unwrap()
        .with_initiator_addr(ALICE_PUBLIC_KEY.clone())
        .with_pricing_mode(txn_pricing_mode)
        .with_chain_name(chain_name)
        .build()
        .unwrap(),
    );
    txn_3.sign(&ALICE_SECRET_KEY);
    let txn_3_hash = txn_3.hash();

    test.fixture.inject_transaction(txn_1).await;
    test.fixture.inject_transaction(txn_2).await;
    test.fixture.inject_transaction(txn_3).await;

    test.fixture
        .run_until_executed_transaction(&txn_1_hash, TEN_SECS)
        .await;
    test.fixture
        .run_until_executed_transaction(&txn_2_hash, TEN_SECS)
        .await;
    test.fixture
        .run_until_executed_transaction(&txn_3_hash, TEN_SECS)
        .await;

    let (_node_id, runner) = test.fixture.network.nodes().iter().next().unwrap();
    let ExecutionInfo {
        block_height: txn_1_block_height,
        execution_result: txn_1_exec_result,
        ..
    } = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_1_hash)
        .expect("Expected transaction to be included in a block.");
    let ExecutionInfo {
        block_height: txn_2_block_height,
        execution_result: txn_2_exec_result,
        ..
    } = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_2_hash)
        .expect("Expected transaction to be included in a block.");
    let ExecutionInfo {
        block_height: txn_3_block_height,
        execution_result: txn_3_exec_result,
        ..
    } = runner
        .main_reactor()
        .storage()
        .read_execution_info(txn_3_hash)
        .expect("Expected transaction to be included in a block.");

    let txn_1_exec_result = txn_1_exec_result.expect("Expected result for txn 1");
    let txn_2_exec_result = txn_2_exec_result.expect("Expected result for txn 2");
    let txn_3_exec_result = txn_3_exec_result.expect("Expected result for txn 3");

    assert!(exec_result_is_success(&txn_1_exec_result));
    assert!(exec_result_is_success(&txn_2_exec_result));
    assert!(exec_result_is_success(&txn_3_exec_result));

    assert_exec_result_cost(
        txn_1_exec_result,
        expected_transfer_cost,
        expected_transfer_gas.into(),
        "gas_holds_accumulate_for_multiple_transactions_in_the_same_block txn1",
    );
    assert_exec_result_cost(
        txn_2_exec_result,
        expected_transfer_cost,
        expected_transfer_gas.into(),
        "gas_holds_accumulate_for_multiple_transactions_in_the_same_block txn2",
    );
    assert_exec_result_cost(
        txn_3_exec_result,
        expected_transfer_cost,
        expected_transfer_gas.into(),
        "gas_holds_accumulate_for_multiple_transactions_in_the_same_block txn3",
    );

    let max_block_height = std::cmp::max(
        std::cmp::max(txn_1_block_height, txn_2_block_height),
        txn_3_block_height,
    );
    let alice_total_holds: U512 = get_balance(
        &test.fixture,
        &ALICE_PUBLIC_KEY,
        Some(max_block_height),
        false,
    )
    .proofs_result()
    .expect("Expected Alice to proof results.")
    .balance_holds()
    .expect("Expected Alice to have holds.")
    .values()
    .map(|block_holds| block_holds.values().copied().sum())
    .sum();
    assert_eq!(
        alice_total_holds,
        expected_transfer_cost * 3,
        "Total holds amount should be equal to the cost of the 3 transactions."
    );

    test.fixture
        .run_until_block_height(max_block_height + 5, ONE_MIN)
        .await;
    let alice_total_holds: U512 = get_balance(&test.fixture, &ALICE_PUBLIC_KEY, None, false)
        .proofs_result()
        .expect("Expected Alice to proof results.")
        .balance_holds()
        .expect("Expected Alice to have holds.")
        .values()
        .map(|block_holds| block_holds.values().copied().sum())
        .sum();
    assert_eq!(
        alice_total_holds,
        U512::from(0),
        "Holds should have expired."
    );
}

#[tokio::test]
async fn gh_5058_regression_custom_payment_with_deploy_variant_works() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // This WASM creates named key called "new_key". Then it would loop endlessly trying to write a
    // value to storage. Eventually it will run out of gas and it should exit causing a revert.
    let base_path = RESOURCES_PATH
        .parent()
        .unwrap()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");

    let payment_amount = U512::from(2_500_000_000u64);

    let txn = {
        let timestamp = Timestamp::now();
        let ttl = TimeDiff::from_seconds(100);
        let gas_price = 1;
        let chain_name = test.chainspec().network_config.name.clone();

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: std::fs::read(base_path.join("gh_5058_regression.wasm"))
                .unwrap()
                .into(),
            args: runtime_args! {
                "amount" => payment_amount,
            },
        };

        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                .unwrap()
                .into(),
            args: runtime_args! {},
        };

        Transaction::Deploy(Deploy::new_signed(
            timestamp,
            ttl,
            gas_price,
            vec![],
            chain_name.clone(),
            payment,
            session,
            &ALICE_SECRET_KEY,
            Some(ALICE_PUBLIC_KEY.clone()),
        ))
    };

    let acct = get_balance(&test.fixture, &ALICE_PUBLIC_KEY, None, true);
    assert!(acct.total_balance().cloned().unwrap() >= payment_amount);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;

    assert_eq!(exec_result.error_message(), None);
}

#[tokio::test]
async fn should_penalize_failed_custom_payment() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // This WASM creates named key called "new_key". Then it would loop endlessly trying to write a
    // value to storage. Eventually it will run out of gas and it should exit causing a revert.
    let base_path = RESOURCES_PATH
        .parent()
        .unwrap()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");

    let payment_amount = U512::from(1_000_000u64);

    let txn = {
        let timestamp = Timestamp::now();
        let ttl = TimeDiff::from_seconds(100);
        let gas_price = 1;
        let chain_name = test.chainspec().network_config.name.clone();

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                .unwrap()
                .into(),
            args: runtime_args! {
                "amount" => payment_amount,
            },
        };

        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                .unwrap()
                .into(),
            args: runtime_args! {
                "this_is_session" => true,
            },
        };

        Transaction::Deploy(Deploy::new_signed(
            timestamp,
            ttl,
            gas_price,
            vec![],
            chain_name.clone(),
            payment,
            session,
            &ALICE_SECRET_KEY,
            Some(ALICE_PUBLIC_KEY.clone()),
        ))
    };

    let acct = get_balance(&test.fixture, &ALICE_PUBLIC_KEY, None, true);
    assert!(acct.total_balance().cloned().unwrap() >= payment_amount);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;

    assert_ne!(exec_result.error_message(), None);

    assert!(exec_result
        .error_message()
        .expect("should have err message")
        .starts_with("Insufficient custom payment"))
}

#[tokio::test]
async fn gh_5082_install_upgrade_should_allow_adding_new_version() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // This WASM creates named key called "new_key". Then it would loop endlessly trying to write a
    // value to storage. Eventually it will run out of gas and it should exit causing a revert.
    let base_path = RESOURCES_PATH
        .parent()
        .unwrap()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");

    let txn_1 = {
        let chain_name = test.chainspec().network_config.name.clone();

        let module_bytes = std::fs::read(base_path.join("do_nothing_stored.wasm")).unwrap();
        let mut txn = Transaction::from(
            TransactionV1Builder::new_session(
                true,
                module_bytes.into(),
                TransactionRuntimeParams::VmCasperV1,
            )
            .with_initiator_addr(ALICE_PUBLIC_KEY.clone())
            .with_pricing_mode(PricingMode::PaymentLimited {
                payment_amount: 100_000_000_000u64,
                gas_price_tolerance: 1,
                standard_payment: true,
            })
            .with_chain_name(chain_name)
            .build()
            .unwrap(),
        );
        txn.sign(&ALICE_SECRET_KEY);
        txn
    };

    let (_txn_hash, _block_height, exec_result_1) = test.send_transaction(txn_1).await;

    assert_eq!(exec_result_1.error_message(), None); // should succeed

    let txn_2 = {
        let chain_name = test.chainspec().network_config.name.clone();

        let module_bytes = std::fs::read(base_path.join("do_nothing_stored.wasm")).unwrap();
        let mut txn = Transaction::from(
            TransactionV1Builder::new_session(
                true,
                module_bytes.into(),
                TransactionRuntimeParams::VmCasperV1,
            )
            .with_initiator_addr(BOB_PUBLIC_KEY.clone())
            .with_pricing_mode(PricingMode::PaymentLimited {
                payment_amount: 100_000_000_000u64,
                gas_price_tolerance: 1,
                // This is the key part of the test: we are using `standard_payment == false` to use
                // session code as payment code. This should fail to add new
                // contract version.
                standard_payment: false,
            })
            .with_chain_name(chain_name)
            .build()
            .unwrap(),
        );
        txn.sign(&BOB_SECRET_KEY);
        txn
    };

    let (_txn_hash, _block_height, exec_result_2) = test.send_transaction(txn_2).await;

    assert_eq!(
        exec_result_2.error_message(),
        Some("ApiError::NotAllowedToAddContractVersion [48]".to_string())
    ); // should not succeed, adding new contract version during payment is not allowed.
}

#[tokio::test]
async fn should_allow_custom_payment() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::NoRefund)
        .with_fee_handling(FeeHandling::NoFee);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    // This WASM creates named key called "new_key". Then it would loop endlessly trying to write a
    // value to storage. Eventually it will run out of gas and it should exit causing a revert.
    let base_path = RESOURCES_PATH
        .parent()
        .unwrap()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");

    let payment_amount = U512::from(2_500_000_000u64);

    let txn = {
        let timestamp = Timestamp::now();
        let ttl = TimeDiff::from_seconds(100);
        let gas_price = 1;
        let chain_name = test.chainspec().network_config.name.clone();

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: std::fs::read(base_path.join("non_standard_payment.wasm"))
                .unwrap()
                .into(),
            args: runtime_args! {
                "amount" => payment_amount,
            },
        };

        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                .unwrap()
                .into(),
            args: runtime_args! {
                "this_is_session" => true,
            },
        };

        Transaction::Deploy(Deploy::new_signed(
            timestamp,
            ttl,
            gas_price,
            vec![],
            chain_name.clone(),
            payment,
            session,
            &ALICE_SECRET_KEY,
            Some(ALICE_PUBLIC_KEY.clone()),
        ))
    };

    let acct = get_balance(&test.fixture, &ALICE_PUBLIC_KEY, None, true);
    assert!(acct.total_balance().cloned().unwrap() >= payment_amount);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;

    assert_eq!(exec_result.error_message(), None);
    assert!(
        exec_result.consumed() > U512::zero(),
        "should have consumed gas"
    );
}

#[tokio::test]
async fn should_allow_native_transfer_v1() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(99, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let transfer_amount = U512::from(100);

    let txn_v1 =
        TransactionV1Builder::new_transfer(transfer_amount, None, CHARLIE_PUBLIC_KEY.clone(), None)
            .unwrap()
            .with_chain_name(CHAIN_NAME)
            .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
            .build()
            .unwrap();
    let payment = txn_v1
        .payment_amount()
        .expect("must have payment amount as txns are using payment_limited");
    let mut txn = Transaction::from(txn_v1);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };

    assert_ne!(
        U512::from(payment),
        result.cost,
        "native transfer costing is system limited"
    );
    let expected_cost: U512 = U512::from(DEFAULT_TRANSFER_COST);
    assert_eq!(result.error_message.as_deref(), None);
    assert_eq!(result.cost, expected_cost);
    assert_eq!(result.transfers.len(), 1, "should have exactly 1 transfer");
}

#[tokio::test]
async fn should_allow_native_burn() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(99, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    let burn_amount = U512::from(100);

    let txn_v1 = TransactionV1Builder::new_burn(burn_amount, None)
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap();
    let payment = txn_v1
        .payment_amount()
        .expect("must have payment amount as txns are using payment_limited");
    let mut txn = Transaction::from(txn_v1);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };
    let expected_cost: U512 = U512::from(payment) * MIN_GAS_PRICE;
    assert_eq!(result.error_message.as_deref(), None);
    assert_eq!(result.cost, expected_cost);
}

#[tokio::test]
async fn should_not_allow_unverified_native_burn() {
    let config = SingleTransactionTestCase::default_test_config()
        .with_pricing_handling(PricingHandling::PaymentLimited)
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(99, 100),
        })
        .with_fee_handling(FeeHandling::PayToProposer)
        .with_gas_hold_balance_handling(HoldBalanceHandling::Accrued);

    let mut test = SingleTransactionTestCase::new(
        ALICE_SECRET_KEY.clone(),
        BOB_SECRET_KEY.clone(),
        CHARLIE_SECRET_KEY.clone(),
        Some(config),
    )
    .await;

    test.fixture
        .run_until_consensus_in_era(ERA_ONE, ONE_MIN)
        .await;

    let burn_amount = U512::from(100);

    let alice_uref_addr =
        get_main_purse(&mut test.fixture, &ALICE_PUBLIC_KEY).expect("should have main purse");
    let alice_purse = URef::new(alice_uref_addr, AccessRights::all());

    let txn_v1 = TransactionV1Builder::new_burn(burn_amount, Some(alice_purse))
        .unwrap()
        .with_chain_name(CHAIN_NAME)
        .with_initiator_addr(PublicKey::from(&**BOB_SECRET_KEY))
        .build()
        .unwrap();
    let price = txn_v1
        .payment_amount()
        .expect("must have payment amount as txns are using payment_limited");
    let mut txn = Transaction::from(txn_v1);
    txn.sign(&BOB_SECRET_KEY);

    let (_txn_hash, _block_height, exec_result) = test.send_transaction(txn).await;
    let ExecutionResult::V2(result) = exec_result else {
        panic!("Expected ExecutionResult::V2 but got {:?}", exec_result);
    };
    let expected_cost: U512 = U512::from(price) * MIN_GAS_PRICE;
    let expected_error = format!("Forged reference: {}", alice_purse);
    assert_eq!(result.error_message, Some(expected_error));
    assert_eq!(result.cost, expected_cost);
}

enum SizingScenario {
    Gas,
    SerializedLength,
}

async fn run_sizing_scenario(sizing_scenario: SizingScenario) {
    let mut rng = TestRng::new();
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let charlie_stake = 300_000_000_000_u64;
    let initial_stakes: Vec<(U512, U512)> = vec![
        (U512::from(u64::MAX), alice_stake.into()),
        (U512::from(u64::MAX), bob_stake.into()),
        (U512::from(u64::MAX), charlie_stake.into()),
    ];

    let secret_keys: Vec<Arc<SecretKey>> = (0..3)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();

    let stakes = secret_keys
        .iter()
        .zip(initial_stakes)
        .map(|(secret_key, stake)| (PublicKey::from(secret_key.as_ref()), stake))
        .collect();

    let mut fixture = TestFixture::new_with_keys(rng, secret_keys, stakes, None).await;

    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let base_path = RESOURCES_PATH
        .parent()
        .unwrap()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");

    let (payment_1, session_1) = match sizing_scenario {
        SizingScenario::Gas => {
            // We create two equally sized deploys, and ensure that they are both
            // executed in the non largest lane by gas limit.
            let gas_limit_for_lane_4 = fixture
                .chainspec
                .transaction_config
                .transaction_v1_config
                .get_max_transaction_gas_limit(4u8);

            let payment = ExecutableDeployItem::ModuleBytes {
                module_bytes: Bytes::new(),
                args: runtime_args! {
                "amount" =>  U512::from(gas_limit_for_lane_4),
                            },
            };

            let session = ExecutableDeployItem::ModuleBytes {
                module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                    .unwrap()
                    .into(),
                args: runtime_args! {},
            };

            (payment, session)
        }
        SizingScenario::SerializedLength => {
            let gas_limit_for_lane_3 = fixture
                .chainspec
                .transaction_config
                .transaction_v1_config
                .get_max_transaction_gas_limit(3u8);

            let payment = ExecutableDeployItem::ModuleBytes {
                module_bytes: Bytes::new(),
                args: runtime_args! {
                    "amount" =>  U512::from(gas_limit_for_lane_3)
                },
            };

            let session = ExecutableDeployItem::ModuleBytes {
                module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                    .unwrap()
                    .into(),
                args: runtime_args! {},
            };

            (payment, session)
        }
    };

    let timestamp = Timestamp::now();
    let ttl = TimeDiff::from_seconds(100);
    let gas_price = 1;
    let chain_name = fixture.chainspec.network_config.name.clone();

    let transaction_1 = Transaction::Deploy(Deploy::new_signed(
        timestamp,
        ttl,
        gas_price,
        vec![],
        chain_name.clone(),
        payment_1,
        session_1,
        &ALICE_SECRET_KEY,
        Some(ALICE_PUBLIC_KEY.clone()),
    ));

    let wasm_lanes = fixture
        .chainspec
        .transaction_config
        .transaction_v1_config
        .wasm_lanes();

    let largest_lane = wasm_lanes
        .iter()
        .max_by(|left, right| {
            left.max_transaction_length
                .cmp(&right.max_transaction_length)
        })
        .map(|definition| definition.id)
        .expect("must have lane id for largest lane");

    let (payment_2, session_2) = match sizing_scenario {
        SizingScenario::Gas => {
            // We create two equally sized deploys, and ensure that they are both
            // executed in the non largest lane by gas limit.
            let gas_limit_for_lane_3 = fixture
                .chainspec
                .transaction_config
                .transaction_v1_config
                .get_max_transaction_gas_limit(3u8);

            let payment = ExecutableDeployItem::ModuleBytes {
                module_bytes: Bytes::new(),
                args: runtime_args! {
                "amount" =>  U512::from(gas_limit_for_lane_3),
                            },
            };

            let session = ExecutableDeployItem::ModuleBytes {
                module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
                    .unwrap()
                    .into(),
                args: runtime_args! {},
            };

            (payment, session)
        }
        SizingScenario::SerializedLength => {
            let largest_lane_gas_limit = fixture
                .chainspec
                .transaction_config
                .transaction_v1_config
                .get_max_transaction_gas_limit(largest_lane);

            let payment = ExecutableDeployItem::ModuleBytes {
                module_bytes: Bytes::new(),
                args: runtime_args! {
                    "amount" =>  U512::from(largest_lane_gas_limit)
                },
            };

            let faucet_fund_amount = U512::from(400_000_000_000_000u64);

            let session = ExecutableDeployItem::ModuleBytes {
                module_bytes: std::fs::read(base_path.join("faucet_stored.wasm"))
                    .unwrap()
                    .into(),
                args: runtime_args! {"id" => 1u64, ARG_AMOUNT => faucet_fund_amount },
            };

            (payment, session)
        }
    };

    let transaction_2 = Transaction::Deploy(Deploy::new_signed(
        timestamp,
        ttl,
        gas_price,
        vec![],
        chain_name.clone(),
        payment_2,
        session_2,
        &ALICE_SECRET_KEY,
        Some(ALICE_PUBLIC_KEY.clone()),
    ));

    // Both deploys are of roughly equal length but should be sized differently based on
    // their payment amount.

    let txn_1 = transaction_1.hash();
    let txn_2 = transaction_2.hash();

    fixture.inject_transaction(transaction_1).await;
    fixture.inject_transaction(transaction_2).await;

    match sizing_scenario {
        SizingScenario::Gas => {
            fixture
                .assert_execution_in_lane(&txn_1, 4u8, TEN_SECS)
                .await;
            fixture
                .assert_execution_in_lane(&txn_2, 3u8, TEN_SECS)
                .await;
        }
        SizingScenario::SerializedLength => {
            fixture
                .assert_execution_in_lane(&txn_1, 3u8, TEN_SECS)
                .await;
            fixture
                .assert_execution_in_lane(&txn_2, largest_lane, TEN_SECS)
                .await;
        }
    }
}

#[tokio::test]
async fn should_correctly_assign_wasm_deploys_in_lanes_for_payment_limited_by_gas_limit() {
    run_sizing_scenario(SizingScenario::Gas).await
}

#[tokio::test]
async fn should_correctly_assign_wasm_deploys_in_lanes_for_payment_limited_by_serialized_length() {
    run_sizing_scenario(SizingScenario::SerializedLength).await
}

#[tokio::test]
async fn should_assign_deploy_to_largest_lane_by_payment_amount_only_in_payment_limited() {
    let mut rng = TestRng::new();
    let alice_stake = 200_000_000_000_u64;
    let bob_stake = 300_000_000_000_u64;
    let charlie_stake = 300_000_000_000_u64;
    let initial_stakes: Vec<(U512, U512)> = vec![
        (U512::from(u64::MAX), alice_stake.into()),
        (U512::from(u64::MAX), bob_stake.into()),
        (U512::from(u64::MAX), charlie_stake.into()),
    ];

    let secret_keys: Vec<Arc<SecretKey>> = (0..3)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();

    let stakes = secret_keys
        .iter()
        .zip(initial_stakes)
        .map(|(secret_key, stake)| (PublicKey::from(secret_key.as_ref()), stake))
        .collect();

    let mut fixture = TestFixture::new_with_keys(rng, secret_keys, stakes, None).await;

    fixture
        .run_until_stored_switch_block_header(ERA_ONE, ONE_MIN)
        .await;

    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    let base_path = RESOURCES_PATH
        .parent()
        .unwrap()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");

    let mut wasm_lanes = fixture
        .chainspec
        .transaction_config
        .transaction_v1_config
        .wasm_lanes()
        .clone();

    wasm_lanes.sort_by(|a, b| {
        a.max_transaction_gas_limit
            .cmp(&b.max_transaction_gas_limit)
    });

    let (smallest_lane_id, smallest_gas_limt, smallest_size_limit_for_deploy) = wasm_lanes
        .first()
        .map(|lane_def| {
            (
                lane_def.id,
                lane_def.max_transaction_gas_limit,
                lane_def.max_transaction_length,
            )
        })
        .expect("must have at least one lane");

    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: runtime_args! {
        "amount" =>  U512::from(smallest_gas_limt),
                    },
    };

    let session = ExecutableDeployItem::ModuleBytes {
        module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
            .unwrap()
            .into(),
        args: runtime_args! {},
    };

    let timestamp = Timestamp::now();
    let ttl = TimeDiff::from_seconds(100);
    let gas_price = 1;
    let chain_name = fixture.chainspec.network_config.name.clone();

    let transaction = Transaction::Deploy(Deploy::new_signed(
        timestamp,
        ttl,
        gas_price,
        vec![],
        chain_name.clone(),
        payment,
        session,
        &ALICE_SECRET_KEY,
        Some(ALICE_PUBLIC_KEY.clone()),
    ));

    let small_txn_hash = transaction.hash();
    let small_txn_size = transaction.serialized_length() as u64;
    assert!(small_txn_size < smallest_size_limit_for_deploy);

    fixture.inject_transaction(transaction).await;

    fixture
        .assert_execution_in_lane(&small_txn_hash, smallest_lane_id, TEN_SECS)
        .await;

    let (largest_lane_id, largest_gas_limt) = wasm_lanes
        .last()
        .map(|lane_def| (lane_def.id, lane_def.max_transaction_gas_limit))
        .expect("must have at least one lane");

    assert_ne!(largest_lane_id, smallest_lane_id);
    assert!(largest_gas_limt > smallest_gas_limt);

    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: runtime_args! {
        "amount" =>  U512::from(largest_gas_limt),
                    },
    };

    let session = ExecutableDeployItem::ModuleBytes {
        module_bytes: std::fs::read(base_path.join("do_nothing.wasm"))
            .unwrap()
            .into(),
        args: runtime_args! {},
    };

    let chain_name = fixture.chainspec.network_config.name.clone();

    let transaction = Transaction::Deploy(Deploy::new_signed(
        timestamp,
        ttl,
        gas_price,
        vec![],
        chain_name.clone(),
        payment,
        session,
        &ALICE_SECRET_KEY,
        Some(ALICE_PUBLIC_KEY.clone()),
    ));

    let largest_txn_hash = transaction.hash();

    let largest_txn_size = transaction.serialized_length() as u64;
    // This is misnomer, its the size of the deploy meant to be in the
    // largest lane.
    assert!(largest_txn_size < smallest_size_limit_for_deploy);

    fixture.inject_transaction(transaction).await;

    fixture
        .assert_execution_in_lane(&largest_txn_hash, largest_lane_id, TEN_SECS)
        .await;
}
