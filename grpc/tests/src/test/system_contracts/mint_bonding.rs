use casperlabs_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_PAYMENT,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::{types::Motes, GenesisAccount};
use casperlabs_types::{
    account::AccountHash,
    bytesrepr::FromBytes,
    mint::{BidPurses, FounderPurses, UnbondingPurses},
    runtime_args, ApiError, CLTyped, ContractHash, RuntimeArgs, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_MINT_BONDING: &str = "mint_bonding.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

const GENESIS_VALIDATOR_STAKE: u64 = 50_000;
const GENESIS_ACCOUNT_STAKE: u64 = 100_000;
const TRANSFER_AMOUNT: u64 = 500_000_000;

const TEST_BOND: &str = "bond";
const TEST_BOND_FROM_MAIN_PURSE: &str = "bond-from-main-purse";
const TEST_SEED_NEW_ACCOUNT: &str = "seed_new_account";
const TEST_UNBOND: &str = "unbond";
const TEST_RELEASE: &str = "release";

const ARG_AMOUNT: &str = "amount";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ACCOUNT_PK: &str = "account_hash";

const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

fn get_value<T: FromBytes + CLTyped>(
    builder: &mut InMemoryWasmTestBuilder,
    contract_hash: ContractHash,
    name: &str,
) -> T {
    let contract = builder
        .get_contract(contract_hash)
        .expect("should have contract");
    let key = contract
        .named_keys()
        .get(name)
        .expect("should have bid purses");
    let stored_value = builder.query(None, *key, &[]).expect("should query");
    let cl_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    let result: T = cl_value.into_t().expect("should convert");
    result
}

#[ignore]
#[test]
fn should_run_successful_bond_and_unbond() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let _default_account = builder
        .get_account(DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let mint = builder.get_mint_contract_hash();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_BOND),
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE)
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bid_purses: BidPurses = get_value(&mut builder, mint, "bid_purses");
    let bid_purse = bid_purses
        .get(&DEFAULT_ACCOUNT_ADDR)
        .expect("should have bid purse");
    assert_eq!(
        builder.get_purse_balance(*bid_purse),
        GENESIS_ACCOUNT_STAKE.into()
    );

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 0);

    //
    // Partial unbond
    //

    let unbond_amount = U512::from(GENESIS_ACCOUNT_STAKE) - 1;

    let exec_request_2 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => unbond_amount,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 1);

    let unbond_list = unbond_purses
        .get(&DEFAULT_ACCOUNT_ADDR)
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, DEFAULT_ACCOUNT_ADDR);
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        unbond_amount
    );

    let unbond_timer_1 = unbond_list[0].expiration_timer;

    //
    //
    //
    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        mint,
        "unbond_timer_advance",
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 1);

    let unbond_list = unbond_purses
        .get(&DEFAULT_ACCOUNT_ADDR)
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, DEFAULT_ACCOUNT_ADDR);
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        unbond_amount
    );

    let unbond_timer_2 = unbond_list[0].expiration_timer;

    assert!(unbond_timer_2 < unbond_timer_1); // expiration timer is decreased

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        mint,
        "slash",
        runtime_args! {
            "validator_account_hashes" => vec![
                DEFAULT_ACCOUNT_ADDR,
            ]
        },
    )
    .build();

    builder.exec(exec_request_4).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    let unbond_list = unbond_purses
        .get(&DEFAULT_ACCOUNT_ADDR)
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 0); // removed unbonds

    let bid_purses: BidPurses = get_value(&mut builder, mint, "bid_purses");

    assert!(bid_purses.is_empty());
}

#[ignore]
#[test]
fn should_fail_bonding_with_insufficient_funds() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_SEED_NEW_ACCOUNT,
            ARG_ACCOUNT_PK => ACCOUNT_1_ADDR,
            ARG_AMOUNT => *DEFAULT_PAYMENT + GENESIS_ACCOUNT_STAKE,
        },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_BOND_FROM_MAIN_PURSE,
            ARG_AMOUNT => *DEFAULT_PAYMENT + GENESIS_ACCOUNT_STAKE,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .commit();

    builder.exec(exec_request_2).commit();

    let response = builder
        .get_exec_response(1)
        .expect("should have a response")
        .to_owned();

    let error_message = utils::get_error_message(response);

    assert!(
        error_message.contains(&format!("{:?}", ApiError::Mint(0))),
        "error: {:?}",
        error_message
    );
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
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_UNBOND,
            ARG_AMOUNT => U512::from(42),
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    builder.exec(exec_request).commit();

    let response = builder
        .get_exec_response(0)
        .expect("should have a response")
        .to_owned();

    let error_message = utils::get_error_message(response);

    // pos::Error::NotBonded => 0
    assert!(
        error_message.contains(&format!("{:?}", ApiError::Mint(9))),
        "error {:?}",
        error_message
    );
}

#[ignore]
#[test]
fn should_release_founder_validator_stake() {
    let validator = AccountHash::new([42; 32]);

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::new(
            validator,
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let mint = builder.get_mint_contract_hash();

    let exec_request_3 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_RELEASE,
            "validator_account_hash" => validator,
        },
    )
    .build();

    // Locked funds
    let founder_purses: FounderPurses = get_value(&mut builder, mint, "founder_purses");
    let (purse, release_status) = founder_purses.get(&validator).unwrap();
    assert_eq!(*release_status, false); // locked
    assert_eq!(
        builder.get_purse_balance(*purse),
        U512::from(GENESIS_VALIDATOR_STAKE)
    );

    builder.exec(exec_request_3).expect_success().commit();

    let founder_purses: FounderPurses = get_value(&mut builder, mint, "founder_purses");

    let (purse, release_status) = founder_purses.get(&validator).unwrap();
    assert_eq!(*release_status, true); // unlocked
    assert_eq!(
        builder.get_purse_balance(*purse),
        U512::from(GENESIS_VALIDATOR_STAKE)
    );
}
