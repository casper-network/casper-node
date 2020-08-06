use casperlabs_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_PAYMENT,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casperlabs_node::{
    components::contract_runtime::core::engine_state::{genesis::POS_BONDING_PURSE, CONV_RATE},
    types::Motes,
    GenesisAccount,
};
use casperlabs_types::{
    account::AccountHash, runtime_args, ApiError, Key, RuntimeArgs, URef, U512,
};

const CONTRACT_POS_BONDING: &str = "pos_bonding.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const ACCOUNT_1_SEED_AMOUNT: u64 = 100_000_000 * 2;
const ACCOUNT_1_STAKE: u64 = 42_000;
const ACCOUNT_1_UNBOND_1: u64 = 22_000;
const ACCOUNT_1_UNBOND_2: u64 = 20_000;

const GENESIS_VALIDATOR_STAKE: u64 = 50_000;
const GENESIS_ACCOUNT_STAKE: u64 = 100_000;
const GENESIS_ACCOUNT_UNBOND_1: u64 = 45_000;
const GENESIS_ACCOUNT_UNBOND_2: u64 = 55_000;

const TEST_BOND: &str = "bond";
const TEST_BOND_FROM_MAIN_PURSE: &str = "bond-from-main-purse";
const TEST_SEED_NEW_ACCOUNT: &str = "seed_new_account";
const TEST_UNBOND: &str = "unbond";

const ARG_AMOUNT: &str = "amount";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ACCOUNT_PK: &str = "account_hash";

fn get_pos_purse_by_name(builder: &InMemoryWasmTestBuilder, purse_name: &str) -> Option<URef> {
    let pos_contract = builder.get_pos_contract();

    pos_contract
        .named_keys()
        .get(purse_name)
        .and_then(Key::as_uref)
        .cloned()
}

fn get_pos_bonding_purse_balance(builder: &InMemoryWasmTestBuilder) -> U512 {
    let purse =
        get_pos_purse_by_name(builder, POS_BONDING_PURSE).expect("should find PoS payment purse");
    builder.get_purse_balance(purse)
}

#[ignore]
#[test]
fn should_run_successful_bond_and_unbond() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::new(
            AccountHash::new([42; 32]),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();
    let result = builder.run_genesis(&run_genesis_request).finish();

    let default_account = builder
        .get_account(DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let pos = builder.get_pos_contract_hash();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_BOND),
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE)
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::from_result(result);

    let result = builder.exec(exec_request_1);
    if !cfg!(feature = "enable-bonding") && result.is_error() {
        return;
    }

    let result = builder.expect_success().commit().finish();

    let exec_response = builder
        .get_exec_response(0)
        .expect("should have exec response");
    let mut genesis_gas_cost = utils::get_exec_costs(exec_response)[0];

    let contract = builder.get_contract(pos).expect("should have contract");

    let lookup_key = format!(
        "v_{}_{}",
        base16::encode_lower(&DEFAULT_ACCOUNT_ADDR.as_bytes()),
        GENESIS_ACCOUNT_STAKE
    );
    assert!(contract.named_keys().contains_key(&lookup_key));

    // Gensis validator [42; 32] bonded 50k, and genesis account bonded 100k inside
    // the test contract
    assert_eq!(
        get_pos_bonding_purse_balance(&builder),
        U512::from(GENESIS_VALIDATOR_STAKE + GENESIS_ACCOUNT_STAKE)
    );

    let exec_request_2 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_SEED_NEW_ACCOUNT,
            ARG_ACCOUNT_PK => ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(ACCOUNT_1_SEED_AMOUNT),
        },
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_BOND_FROM_MAIN_PURSE),
            ARG_AMOUNT => U512::from(ACCOUNT_1_STAKE),
        },
    )
    .build();

    // Create new account (from genesis funds) and bond with it
    let mut builder = InMemoryWasmTestBuilder::from_result(result);
    let result = builder
        .exec(exec_request_2)
        .expect_success()
        .commit()
        .exec(exec_request_3)
        .expect_success()
        .commit()
        .finish();

    let exec_response = builder
        .get_exec_response(0)
        .expect("should have exec response");
    genesis_gas_cost = genesis_gas_cost + utils::get_exec_costs(exec_response)[0];

    let account_1 = builder
        .get_account(ACCOUNT_1_ADDR)
        .expect("should get account 1");

    let pos = builder.get_pos_contract_hash();

    // Verify that genesis account is in validator queue
    let contract = builder.get_contract(pos).expect("should have contract");

    let lookup_key = format!(
        "v_{}_{}",
        base16::encode_lower(ACCOUNT_1_ADDR.as_bytes()),
        ACCOUNT_1_STAKE
    );
    assert!(contract.named_keys().contains_key(&lookup_key));

    // Gensis validator [42; 32] bonded 50k, and genesis account bonded 100k inside
    // the test contract
    let pos_bonding_purse_balance = get_pos_bonding_purse_balance(&builder);
    assert_eq!(
        pos_bonding_purse_balance,
        U512::from(GENESIS_VALIDATOR_STAKE + GENESIS_ACCOUNT_STAKE + ACCOUNT_1_STAKE)
    );

    //
    // Stage 2a - Account 1 unbonds by decreasing less than 50% (and is still in the
    // queue)
    //
    let exec_request_4 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => Some(U512::from(ACCOUNT_1_UNBOND_1)),
        },
    )
    .build();
    let account_1_bal_before = builder.get_purse_balance(account_1.main_purse());
    let mut builder = InMemoryWasmTestBuilder::from_result(result);
    let result = builder
        .exec(exec_request_4)
        .expect_success()
        .commit()
        .finish();

    let account_1_bal_after = builder.get_purse_balance(account_1.main_purse());
    let exec_response = builder
        .get_exec_response(0)
        .expect("should have exec response");
    let gas_cost_b = Motes::from_gas(utils::get_exec_costs(exec_response)[0], CONV_RATE)
        .expect("should convert");

    assert_eq!(
        account_1_bal_after,
        account_1_bal_before - gas_cost_b.value() + ACCOUNT_1_UNBOND_1,
    );

    // POS bonding purse is decreased
    assert_eq!(
        get_pos_bonding_purse_balance(&builder),
        U512::from(GENESIS_VALIDATOR_STAKE + GENESIS_ACCOUNT_STAKE + ACCOUNT_1_UNBOND_2)
    );

    let pos_contract = builder.get_pos_contract();

    let lookup_key = format!(
        "v_{}_{}",
        base16::encode_lower(ACCOUNT_1_ADDR.as_bytes()),
        ACCOUNT_1_STAKE
    );
    assert!(!pos_contract.named_keys().contains_key(&lookup_key));

    let lookup_key = format!(
        "v_{}_{}",
        base16::encode_lower(ACCOUNT_1_ADDR.as_bytes()),
        ACCOUNT_1_UNBOND_2
    );
    // Account 1 is still tracked anymore in the bonding queue with different uref
    // name
    assert!(pos_contract.named_keys().contains_key(&lookup_key));

    //
    // Stage 2b - Genesis unbonds by decreasing less than 50% (and is still in the
    // queue)
    //
    // Genesis account unbonds less than 50% of his stake
    let exec_request_5 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => Some(U512::from(GENESIS_ACCOUNT_UNBOND_1)),
        },
    )
    .build();
    let mut builder = InMemoryWasmTestBuilder::from_result(result);
    let result = builder
        .exec(exec_request_5)
        .expect_success()
        .commit()
        .finish();

    let exec_response = builder
        .get_exec_response(0)
        .expect("should have exec response");
    genesis_gas_cost = genesis_gas_cost + utils::get_exec_costs(exec_response)[0];

    assert_eq!(
        builder.get_purse_balance(default_account.main_purse()),
        U512::from(
            DEFAULT_ACCOUNT_INITIAL_BALANCE
                - Motes::from_gas(genesis_gas_cost, CONV_RATE)
                    .expect("should convert")
                    .value()
                    .as_u64()
                - ACCOUNT_1_SEED_AMOUNT
                - GENESIS_ACCOUNT_UNBOND_2
        ),
    );

    // POS bonding purse is further decreased
    assert_eq!(
        get_pos_bonding_purse_balance(&builder),
        U512::from(GENESIS_VALIDATOR_STAKE + GENESIS_ACCOUNT_UNBOND_2 + ACCOUNT_1_UNBOND_2)
    );

    //
    // Stage 3a - Fully unbond account1 with Some(TOTAL_AMOUNT)
    //
    let account_1_bal_before = builder.get_purse_balance(account_1.main_purse());

    let exec_request_6 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => Some(U512::from(ACCOUNT_1_UNBOND_2)),
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::from_result(result);
    let result = builder
        .exec(exec_request_6)
        .expect_success()
        .commit()
        .finish();

    let account_1_bal_after = builder.get_purse_balance(account_1.main_purse());
    let exec_response = builder
        .get_exec_response(0)
        .expect("should have exec response");
    let gas_cost_b = Motes::from_gas(utils::get_exec_costs(exec_response)[0], CONV_RATE)
        .expect("should convert");

    assert_eq!(
        account_1_bal_after,
        account_1_bal_before - gas_cost_b.value() + ACCOUNT_1_UNBOND_2,
    );

    // POS bonding purse contains now genesis validator (50k) + genesis account
    // (55k)
    assert_eq!(
        get_pos_bonding_purse_balance(&builder),
        U512::from(GENESIS_VALIDATOR_STAKE + GENESIS_ACCOUNT_UNBOND_2)
    );

    let pos_contract = builder.get_pos_contract();

    let lookup_key = format!(
        "v_{}_{}",
        base16::encode_lower(ACCOUNT_1_ADDR.as_bytes()),
        ACCOUNT_1_UNBOND_2
    );
    // Account 1 isn't tracked anymore in the bonding queue
    assert!(!pos_contract.named_keys().contains_key(&lookup_key));

    //
    // Stage 3b - Fully unbond account1 with Some(TOTAL_AMOUNT)
    //

    // Genesis account unbonds less than 50% of his stake
    let exec_request_7 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => None as Option<U512>
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::from_result(result);
    let result = builder
        .exec(exec_request_7)
        .expect_success()
        .commit()
        .finish();

    let exec_response = builder
        .get_exec_response(0)
        .expect("should have exec response");
    genesis_gas_cost = genesis_gas_cost + utils::get_exec_costs(exec_response)[0];

    // Back to original after funding account1's pursee
    assert_eq!(
        result
            .builder()
            .get_purse_balance(default_account.main_purse()),
        U512::from(
            DEFAULT_ACCOUNT_INITIAL_BALANCE
                - Motes::from_gas(genesis_gas_cost, CONV_RATE)
                    .expect("should convert")
                    .value()
                    .as_u64()
                - ACCOUNT_1_SEED_AMOUNT
        )
    );

    // Final balance after two full unbonds is the initial bond valuee
    assert_eq!(
        get_pos_bonding_purse_balance(&builder),
        U512::from(GENESIS_VALIDATOR_STAKE)
    );

    let pos_contract = builder.get_pos_contract();
    let lookup_key = format!(
        "v_{}_{}",
        base16::encode_lower(&DEFAULT_ACCOUNT_ADDR.as_bytes()),
        GENESIS_ACCOUNT_UNBOND_2
    );
    // Genesis is still tracked anymore in the bonding queue with different uref
    // name
    assert!(!pos_contract.named_keys().contains_key(&lookup_key));

    //
    // Final checks on validator queue
    //

    // Account 1 is still tracked anymore in the bonding queue with any amount
    // suffix
    assert_eq!(
        pos_contract
            .named_keys()
            .iter()
            .filter(|(key, _)| key.starts_with(&format!(
                "v_{}",
                base16::encode_lower(&DEFAULT_ACCOUNT_ADDR.as_bytes())
            )))
            .count(),
        0
    );
    assert_eq!(
        pos_contract
            .named_keys()
            .iter()
            .filter(|(key, _)| key.starts_with(&format!(
                "v_{}",
                base16::encode_lower(ACCOUNT_1_ADDR.as_bytes())
            )))
            .count(),
        0
    );
    // only genesis validator is still in the queue
    assert_eq!(
        pos_contract
            .named_keys()
            .iter()
            .filter(|(key, _)| key.starts_with("v_"))
            .count(),
        1
    );
}

#[ignore]
#[test]
fn should_fail_bonding_with_insufficient_funds() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::new(
            AccountHash::new([42; 32]),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_SEED_NEW_ACCOUNT,
            ARG_ACCOUNT_PK => ACCOUNT_1_ADDR,
            ARG_AMOUNT => *DEFAULT_PAYMENT + GENESIS_ACCOUNT_STAKE,
        },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_BOND_FROM_MAIN_PURSE,
            ARG_AMOUNT => *DEFAULT_PAYMENT + GENESIS_ACCOUNT_STAKE,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    builder.exec(exec_request_1).commit();

    builder.exec(exec_request_2).commit();

    let result = builder.finish();

    let response = result
        .builder()
        .get_exec_response(1)
        .expect("should have a response")
        .to_owned();

    let error_message = utils::get_error_message(response);

    if !cfg!(feature = "enable-bonding") {
        assert!(
            error_message.contains(&format!("{:?}", ApiError::Unhandled)),
            "error is {:?}",
            error_message
        );
    } else {
        // pos::Error::BondTransferFailed => 8
        assert!(error_message.contains(&format!("{:?}", ApiError::ProofOfStake(8))));
    }
}

#[ignore]
#[test]
fn should_fail_unbonding_validator_without_bonding_first() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::new(
            AccountHash::new([42; 32]),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let exec_request = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_UNBOND,
            ARG_AMOUNT => Some(U512::from(42)),
        },
    )
    .build();

    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&run_genesis_request)
        .exec(exec_request)
        .commit()
        .finish();

    let response = result
        .builder()
        .get_exec_response(0)
        .expect("should have a response")
        .to_owned();

    let error_message = utils::get_error_message(response);

    if !cfg!(feature = "enable-bonding") {
        assert!(error_message.contains(&format!("{:?}", ApiError::Unhandled)));
    } else {
        // pos::Error::NotBonded => 0
        assert!(error_message.contains(&format!("{:?}", ApiError::ProofOfStake(0))));
    }
}
