use casperlabs_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::{
    crypto::asymmetric_key::{PublicKey, SecretKey},
    types::Motes,
    GenesisAccount,
};
use casperlabs_types::{
    account::AccountHash,
    auction::{BidPurses, UnbondingPurses, DEFAULT_UNBONDING_DELAY, INITIAL_ERA_ID},
    bytesrepr::FromBytes,
    runtime_args, ApiError, CLTyped, ContractHash, RuntimeArgs, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_MINT_BONDING: &str = "mint_bonding.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";

const GENESIS_VALIDATOR_STAKE: u64 = 50_000;
const GENESIS_ACCOUNT_STAKE: u64 = 100_000;
const TRANSFER_AMOUNT: u64 = 500_000_000;

const TEST_BOND: &str = "bond";
const TEST_BOND_FROM_MAIN_PURSE: &str = "bond-from-main-purse";
const TEST_SEED_NEW_ACCOUNT: &str = "seed_new_account";
const TEST_UNBOND: &str = "unbond";

const ARG_AMOUNT: &str = "amount";
const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ACCOUNT_HASH: &str = "account_hash";
const ARG_RUN_AUCTION: &str = "run_auction";

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
fn should_run_successful_bond_and_unbond_and_slashing() {
    let default_public_key_arg = casperlabs_types::PublicKey::from(*DEFAULT_ACCOUNT_PUBLIC_KEY);
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let _default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let mint = builder.get_mint_contract_hash();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_BOND),
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE),
            ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bid_purses: BidPurses = get_value(&mut builder, mint, "bid_purses");
    let bid_purse = bid_purses
        .get(&casperlabs_types::PublicKey::from(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
        ))
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
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => unbond_amount,
            ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 1);

    let unbond_list = unbond_purses
        .get(&casperlabs_types::PublicKey::from(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
        ))
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, default_public_key_arg,);
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        U512::zero(),
    );

    assert_eq!(
        unbond_list[0].era_of_withdrawal as usize,
        INITIAL_ERA_ID as usize + DEFAULT_UNBONDING_DELAY as usize
    );

    let unbond_era_1 = unbond_list[0].era_of_withdrawal;

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        mint,
        "process_unbond_requests",
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 1);

    let unbond_list = unbond_purses
        .get(&casperlabs_types::PublicKey::from(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
        ))
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, default_public_key_arg,);
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        U512::zero(),
    );
    assert_eq!(unbond_list[0].amount, unbond_amount,);

    let unbond_era_2 = unbond_list[0].era_of_withdrawal;

    assert_eq!(unbond_era_2, unbond_era_1);

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        mint,
        "slash",
        runtime_args! {
            "validator_public_keys" => vec![
               default_public_key_arg,
            ]
        },
    )
    .build();

    builder.exec(exec_request_4).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    let unbond_list = unbond_purses
        .get(&casperlabs_types::PublicKey::from(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
        ))
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 0); // removed unbonds

    let bid_purses: BidPurses = get_value(&mut builder, mint, "bid_purses");

    assert!(bid_purses.is_empty());
}

#[ignore]
#[test]
fn should_fail_bonding_with_insufficient_funds() {
    let account_1_secret_key: SecretKey = SecretKey::new_ed25519([123; 32]);
    let account_1_public_key: PublicKey = PublicKey::from(&account_1_secret_key);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_SEED_NEW_ACCOUNT,
            ARG_ACCOUNT_HASH => account_1_public_key.to_account_hash(),
            ARG_AMOUNT => *DEFAULT_PAYMENT + GENESIS_ACCOUNT_STAKE,
        },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        account_1_public_key.to_account_hash(),
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_BOND_FROM_MAIN_PURSE,
            ARG_AMOUNT => *DEFAULT_PAYMENT + GENESIS_ACCOUNT_STAKE,
            ARG_PUBLIC_KEY => casperlabs_types::PublicKey::from(account_1_public_key),
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
    let account_1_secret_key = SecretKey::new_ed25519([42; 32]);
    let account_1_public_key = PublicKey::from(&account_1_secret_key);

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::with_public_key(
            account_1_public_key,
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => TEST_UNBOND,
            ARG_AMOUNT => U512::from(42),
            ARG_PUBLIC_KEY => casperlabs_types::PublicKey::from(account_1_public_key),
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
fn should_run_successful_bond_and_unbond_with_release() {
    let default_public_key_arg = casperlabs_types::PublicKey::from(*DEFAULT_ACCOUNT_PUBLIC_KEY);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let _default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let mint = builder.get_mint_contract_hash();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_BOND),
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE),
            ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bid_purses: BidPurses = get_value(&mut builder, mint, "bid_purses");
    let bid_purse = bid_purses
        .get(&default_public_key_arg)
        .expect("should have bid purse");
    assert_eq!(
        builder.get_purse_balance(*bid_purse),
        GENESIS_ACCOUNT_STAKE.into()
    );

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 0);

    //
    // Advance era by calling run_auction
    //
    let run_auction_request_1 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_RUN_AUCTION,
        },
    )
    .build();

    builder
        .exec(run_auction_request_1)
        .commit()
        .expect_success();

    //
    // Partial unbond
    //

    let unbond_amount = U512::from(GENESIS_ACCOUNT_STAKE) - 1;

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => String::from(TEST_UNBOND),
            ARG_AMOUNT => unbond_amount,
            ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 1);

    let unbond_list = unbond_purses
        .get(&casperlabs_types::PublicKey::from(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
        ))
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, default_public_key_arg,);
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        U512::zero(),
    );

    let unbond_purse = unbond_list[0].purse; // We'll transfer amount there

    assert_eq!(
        unbond_list[0].era_of_withdrawal as usize,
        INITIAL_ERA_ID as usize + 1 + DEFAULT_UNBONDING_DELAY as usize
    );

    let unbond_era_1 = unbond_list[0].era_of_withdrawal;

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        mint,
        "process_unbond_requests",
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    assert_eq!(unbond_purses.len(), 1);

    let unbond_list = unbond_purses
        .get(&default_public_key_arg)
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, default_public_key_arg,);

    assert_ne!(
        unbond_list[0].purse,
        *bid_purse // unbond purse is different than bid purse
    );
    assert_eq!(
        unbond_list[0].purse,
        unbond_purse, // unbond purse is not changed
    );
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        U512::zero(), // Not paid yet
    );

    let unbond_era_2 = unbond_list[0].era_of_withdrawal;

    assert_eq!(unbond_era_2, unbond_era_1); // era of withdrawal didn't change since first run

    //
    // Advance state to hit the unbonding period
    //

    for _ in 0..DEFAULT_UNBONDING_DELAY {
        let run_auction_request_1 = ExecuteRequestBuilder::standard(
            SYSTEM_ADDR,
            CONTRACT_AUCTION_BIDS,
            runtime_args! {
                ARG_ENTRY_POINT => ARG_RUN_AUCTION,
            },
        )
        .build();

        builder
            .exec(run_auction_request_1)
            .commit()
            .expect_success();
    }

    // Should pay out

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        mint,
        "process_unbond_requests",
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_4).expect_success().commit();

    assert_eq!(builder.get_purse_balance(unbond_purse), unbond_amount);

    let unbond_purses: UnbondingPurses = get_value(&mut builder, mint, "unbonding_purses");
    let unbond_list = unbond_purses
        .get(&casperlabs_types::PublicKey::from(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
        ))
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 0); // removed unbonds

    let bid_purses: BidPurses = get_value(&mut builder, mint, "bid_purses");

    assert!(!bid_purses.is_empty());
    assert_eq!(
        builder.get_purse_balance(
            *bid_purses
                .get(&default_public_key_arg)
                .expect("should have unbond")
        ),
        U512::from(GENESIS_ACCOUNT_STAKE) - unbond_amount, // remaining funds
    );
}
