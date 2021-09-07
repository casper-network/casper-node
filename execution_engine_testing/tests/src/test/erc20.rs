use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{
    engine_state::Error as CoreError, execution::Error as ExecError,
};
use casper_types::{
    account::AccountHash, runtime_args, system::mint, ApiError, ContractHash, Key, PublicKey,
    RuntimeArgs, SecretKey, U512,
};

const CONTRACT_ERC20_TEST: &str = "erc20_test.wasm";
const CONTRACT_ERC20_TEST_CALL: &str = "erc20_test_call.wasm";
const NAME_KEY: &str = "name";
const SYMBOL_KEY: &str = "symbol";
const ERC20_CONTRACT_KEY: &str = "contract";
const DECIMALS_KEY: &str = "decimals";
const TOTAL_SUPPLY_KEY: &str = "total_supply";

const ERROR_INSUFFICIENT_BALANCE: u16 = 1;
const ERROR_INSUFFICIENT_ALLOWANCE: u16 = 2;

const TOKEN_NAME: &str = "CasperTest";
const TOKEN_SYMBOL: &str = "CSPRT";
const TOKEN_DECIMALS: u8 = 100;
const TOKEN_TOTAL_SUPPLY: u64 = 1_000_000_000;

const METHOD_TRANSFER: &str = "transfer";
const ARG_AMOUNT: &str = "amount";
const ARG_RECIPIENT: &str = "recipient";

const METHOD_APPROVE: &str = "approve";
const ARG_OWNER: &str = "owner";
const ARG_SPENDER: &str = "spender";

const METHOD_TRANSFER_FROM: &str = "transfer_from";

const CHECK_BALANCE_OF_ENTRYPOINT: &str = "check_balance_of";
const CHECK_ALLOWANCE_OF_ENTRYPOINT: &str = "check_allowance_of";

const ARG_TOKEN_CONTRACT: &str = "token_contract";
const ARG_ADDRESS: &str = "address";
const RESULT_KEY: &str = "result";
const ERC20_TEST_CALL_KEY: &str = "erc20_test_call";

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes(&[221u8; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_1_PUBLIC_KEY.to_account_hash());

static ACCOUNT_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes(&[212u8; 32]).unwrap());
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_2_SECRET_KEY));
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_2_PUBLIC_KEY.to_account_hash());

const TRANSFER_AMOUNT_1: u64 = 200_001;
const TRANSFER_AMOUNT_2: u64 = 19_999;
const ALLOWANCE_AMOUNT_1: u64 = 456_789;
const ALLOWANCE_AMOUNT_2: u64 = 87_654;

fn setup() -> (InMemoryWasmTestBuilder, ContractHash) {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let id: Option<u64> = None;
    let transfer_1_args = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_1_ADDR,
        mint::ARG_AMOUNT => MINIMUM_ACCOUNT_CREATION_BALANCE,
        mint::ARG_ID => id,
    };
    let transfer_2_args = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_2_ADDR,
        mint::ARG_AMOUNT => MINIMUM_ACCOUNT_CREATION_BALANCE,
        mint::ARG_ID => id,
    };

    let transfer_request_1 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_1_args).build();
    let transfer_request_2 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_2_args).build();

    let install_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ERC20_TEST,
        RuntimeArgs::default(),
    )
    .build();
    let install_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ERC20_TEST_CALL,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(transfer_request_1).expect_success().commit();
    builder.exec(transfer_request_2).expect_success().commit();
    builder.exec(install_request_1).expect_success().commit();
    builder.exec(install_request_2).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let erc20_contract_hash = account
        .named_keys()
        .get(ERC20_CONTRACT_KEY)
        .and_then(Key::as_hash)
        .map(|hash| ContractHash::new(*hash))
        .expect("should have contract hash");

    (builder, erc20_contract_hash)
}

#[ignore]
#[test]
fn should_have_queryable_properties() {
    let (mut builder, erc20_contract) = setup();

    let name: String = builder.get_value(erc20_contract, NAME_KEY);
    assert_eq!(name, TOKEN_NAME);

    let symbol: String = builder.get_value(erc20_contract, SYMBOL_KEY);
    assert_eq!(symbol, TOKEN_SYMBOL);

    let decimals: u8 = builder.get_value(erc20_contract, DECIMALS_KEY);
    assert_eq!(decimals, TOKEN_DECIMALS);

    let total_supply: U512 = builder.get_value(erc20_contract, TOTAL_SUPPLY_KEY);
    assert_eq!(total_supply, U512::from(TOKEN_TOTAL_SUPPLY));

    let owner_balance = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(owner_balance, total_supply);
}

fn erc20_check_balance_of(builder: &mut InMemoryWasmTestBuilder, address: AccountHash) -> U512 {
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let erc20_contract_hash = account
        .named_keys()
        .get(ERC20_CONTRACT_KEY)
        .and_then(Key::as_hash)
        .map(|hash| ContractHash::new(*hash))
        .expect("should have test contract hash");
    let erc20_test_contract_hash = account
        .named_keys()
        .get(ERC20_TEST_CALL_KEY)
        .and_then(Key::as_hash)
        .map(|hash| ContractHash::new(*hash))
        .expect("should have test contract hash");

    let check_balance_args = runtime_args! {
        ARG_TOKEN_CONTRACT => erc20_contract_hash,
        ARG_ADDRESS => address,
    };
    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        erc20_test_contract_hash,
        CHECK_BALANCE_OF_ENTRYPOINT,
        check_balance_args,
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    builder.get_value(erc20_test_contract_hash, RESULT_KEY)
}

fn erc20_check_allowance_of(
    builder: &mut InMemoryWasmTestBuilder,
    owner: AccountHash,
    spender: AccountHash,
) -> U512 {
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let erc20_contract_hash = account
        .named_keys()
        .get(ERC20_CONTRACT_KEY)
        .and_then(Key::as_hash)
        .map(|hash| ContractHash::new(*hash))
        .expect("should have test contract hash");
    let erc20_test_contract_hash = account
        .named_keys()
        .get(ERC20_TEST_CALL_KEY)
        .and_then(Key::as_hash)
        .map(|hash| ContractHash::new(*hash))
        .expect("should have test contract hash");

    let check_balance_args = runtime_args! {
        ARG_TOKEN_CONTRACT => erc20_contract_hash,
        ARG_OWNER => owner,
        ARG_SPENDER => spender,
    };
    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        erc20_test_contract_hash,
        CHECK_ALLOWANCE_OF_ENTRYPOINT,
        check_balance_args,
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    builder.get_value(erc20_test_contract_hash, RESULT_KEY)
}

#[ignore]
#[test]
fn should_transfer_from_account_to_account() {
    let (mut builder, erc20_contract) = setup();

    let transfer_amount_1 = U512::from(TRANSFER_AMOUNT_1);
    let transfer_amount_2 = U512::from(TRANSFER_AMOUNT_2);

    let transfer_1_sender = *DEFAULT_ACCOUNT_ADDR;
    let erc20_transfer_1_args = runtime_args! {
        ARG_RECIPIENT => *ACCOUNT_1_ADDR,
        ARG_AMOUNT => transfer_amount_1,
    };

    let transfer_2_sender = *ACCOUNT_1_ADDR;
    let erc20_transfer_2_args = runtime_args! {
        ARG_RECIPIENT => *ACCOUNT_2_ADDR,
        ARG_AMOUNT => transfer_amount_2,
    };

    let sender_balance_before = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_ne!(sender_balance_before, U512::zero());

    let account_1_balance_before = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert_eq!(account_1_balance_before, U512::zero());

    let account_2_balance_before = erc20_check_balance_of(&mut builder, *ACCOUNT_2_ADDR);
    assert_eq!(account_2_balance_before, U512::zero());

    let token_transfer_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        transfer_1_sender,
        erc20_contract,
        METHOD_TRANSFER,
        erc20_transfer_1_args,
    )
    .build();

    builder
        .exec(token_transfer_request_1)
        .expect_success()
        .commit();

    let account_1_balance_after = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert_eq!(account_1_balance_after, transfer_amount_1);
    let account_1_balance_before = account_1_balance_after;

    let sender_balance_after = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(
        sender_balance_after,
        sender_balance_before - transfer_amount_1
    );
    let sender_balance_before = sender_balance_after;

    let token_transfer_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        transfer_2_sender,
        erc20_contract,
        METHOD_TRANSFER,
        erc20_transfer_2_args,
    )
    .build();

    builder
        .exec(token_transfer_request_2)
        .expect_success()
        .commit();

    let sender_balance_after = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(sender_balance_after, sender_balance_before);

    let account_1_balance_after = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert!(account_1_balance_after < account_1_balance_before);
    assert_eq!(
        account_1_balance_after,
        transfer_amount_1 - transfer_amount_2
    );

    let account_2_balance_after = erc20_check_balance_of(&mut builder, *ACCOUNT_2_ADDR);
    assert_eq!(account_2_balance_after, transfer_amount_2);
}

#[ignore]
#[test]
fn should_transfer_full_owned_amount() {
    let (mut builder, erc20_contract) = setup();

    let initial_supply = U512::from(TOKEN_TOTAL_SUPPLY);
    let transfer_amount_1 = initial_supply;

    let transfer_1_sender = *DEFAULT_ACCOUNT_ADDR;
    let erc20_transfer_1_args = runtime_args! {
        ARG_RECIPIENT => *ACCOUNT_1_ADDR,
        ARG_AMOUNT => transfer_amount_1,
    };

    let owner_balance_before = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(owner_balance_before, initial_supply);

    let account_1_balance_before = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert_eq!(account_1_balance_before, U512::zero());

    let token_transfer_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        transfer_1_sender,
        erc20_contract,
        METHOD_TRANSFER,
        erc20_transfer_1_args,
    )
    .build();

    builder
        .exec(token_transfer_request_1)
        .expect_success()
        .commit();

    let account_1_balance_after = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert_eq!(account_1_balance_after, transfer_amount_1);

    let owner_balance_after = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(owner_balance_after, U512::zero());

    let total_supply: U512 = builder.get_value(erc20_contract, TOTAL_SUPPLY_KEY);
    assert_eq!(total_supply, initial_supply);
}

#[ignore]
#[test]
fn should_not_transfer_more_than_owned_balance() {
    let (mut builder, erc20_contract) = setup();

    let initial_supply = U512::from(TOKEN_TOTAL_SUPPLY);
    let transfer_amount = initial_supply + U512::one();

    let transfer_1_sender = *DEFAULT_ACCOUNT_ADDR;
    let erc20_transfer_1_args = runtime_args! {
        ARG_RECIPIENT => *ACCOUNT_1_ADDR,
        ARG_AMOUNT => transfer_amount,
    };

    let owner_balance_before = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(owner_balance_before, initial_supply);
    assert!(transfer_amount > owner_balance_before);

    let account_1_balance_before = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert_eq!(account_1_balance_before, U512::zero());

    let token_transfer_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        transfer_1_sender,
        erc20_contract,
        METHOD_TRANSFER,
        erc20_transfer_1_args,
    )
    .build();

    builder.exec(token_transfer_request_1).commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, CoreError::Exec(ExecError::Revert(ApiError::User(user_error))) if user_error == ERROR_INSUFFICIENT_BALANCE),
        "{:?}",
        error
    );

    let account_1_balance_after = erc20_check_balance_of(&mut builder, *ACCOUNT_1_ADDR);
    assert_eq!(account_1_balance_after, account_1_balance_before);

    let owner_balance_after = erc20_check_balance_of(&mut builder, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(owner_balance_after, initial_supply);

    let total_supply: U512 = builder.get_value(erc20_contract, TOTAL_SUPPLY_KEY);
    assert_eq!(total_supply, initial_supply);
}

#[ignore]
#[test]
fn should_approve_funds() {
    let (mut builder, erc20_contract) = setup();

    let initial_supply = U512::from(TOKEN_TOTAL_SUPPLY);
    let allowance_amount_1 = U512::from(ALLOWANCE_AMOUNT_1);
    let allowance_amount_2 = U512::from(ALLOWANCE_AMOUNT_2);

    let owner = *DEFAULT_ACCOUNT_ADDR;
    let spender = *ACCOUNT_1_ADDR;
    let erc20_approve_1_args = runtime_args! {
        ARG_SPENDER => spender,
        ARG_AMOUNT => allowance_amount_1,
    };
    let erc20_approve_2_args = runtime_args! {
        ARG_SPENDER => spender,
        ARG_AMOUNT => allowance_amount_2,
    };

    let spender_allowance_before = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(spender_allowance_before, U512::zero());

    let approve_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        owner,
        erc20_contract,
        METHOD_APPROVE,
        erc20_approve_1_args,
    )
    .build();

    let approve_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        owner,
        erc20_contract,
        METHOD_APPROVE,
        erc20_approve_2_args,
    )
    .build();

    builder.exec(approve_request_1).expect_success().commit();

    {
        let account_1_allowance_after = erc20_check_allowance_of(&mut builder, owner, spender);
        assert_eq!(account_1_allowance_after, allowance_amount_1);

        let total_supply: U512 = builder.get_value(erc20_contract, TOTAL_SUPPLY_KEY);
        assert_eq!(total_supply, initial_supply);
    }

    // Approve overwrites existing amount rather than increase it

    builder.exec(approve_request_2).expect_success().commit();

    let account_1_allowance_after = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(account_1_allowance_after, allowance_amount_2);

    let total_supply: U512 = builder.get_value(erc20_contract, TOTAL_SUPPLY_KEY);
    assert_eq!(total_supply, initial_supply);
}

#[ignore]
#[test]
fn should_not_transfer_from_without_enough_allowance() {
    let (mut builder, erc20_contract) = setup();

    let allowance_amount_1 = U512::from(ALLOWANCE_AMOUNT_1);
    let transfer_from_amount_1 = allowance_amount_1 + U512::one();

    let owner = *DEFAULT_ACCOUNT_ADDR;
    let spender = *ACCOUNT_1_ADDR;

    let erc20_approve_args = runtime_args! {
        ARG_OWNER => owner,
        ARG_SPENDER => spender,
        ARG_AMOUNT => allowance_amount_1,
    };
    let erc20_transfer_from_args = runtime_args! {
        ARG_OWNER => owner,
        ARG_RECIPIENT => spender,
        ARG_AMOUNT => transfer_from_amount_1,
    };

    let spender_allowance_before = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(spender_allowance_before, U512::zero());

    let approve_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        owner,
        erc20_contract,
        METHOD_APPROVE,
        erc20_approve_args,
    )
    .build();

    let transfer_from_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        owner,
        erc20_contract,
        METHOD_TRANSFER_FROM,
        erc20_transfer_from_args,
    )
    .build();

    builder.exec(approve_request_1).expect_success().commit();

    let account_1_allowance_after = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(account_1_allowance_after, allowance_amount_1);

    builder.exec(transfer_from_request_1).commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, CoreError::Exec(ExecError::Revert(ApiError::User(user_error))) if user_error == ERROR_INSUFFICIENT_ALLOWANCE),
        "{:?}",
        error
    );
}

#[ignore]
#[test]
fn should_transfer_from_from_account_to_account() {
    let (mut builder, erc20_contract) = setup();

    let initial_supply = U512::from(TOKEN_TOTAL_SUPPLY);
    let allowance_amount_1 = U512::from(ALLOWANCE_AMOUNT_1);
    let transfer_from_amount_1 = allowance_amount_1;

    let owner = *DEFAULT_ACCOUNT_ADDR;
    let spender = *ACCOUNT_1_ADDR;

    let erc20_approve_args = runtime_args! {
        ARG_OWNER => owner,
        ARG_SPENDER => spender,
        ARG_AMOUNT => allowance_amount_1,
    };
    let erc20_transfer_from_args = runtime_args! {
        ARG_OWNER => owner,
        ARG_RECIPIENT => spender,
        ARG_AMOUNT => transfer_from_amount_1,
    };

    let spender_allowance_before = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(spender_allowance_before, U512::zero());

    let approve_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        owner,
        erc20_contract,
        METHOD_APPROVE,
        erc20_approve_args,
    )
    .build();

    let transfer_from_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        spender,
        erc20_contract,
        METHOD_TRANSFER_FROM,
        erc20_transfer_from_args,
    )
    .build();

    builder.exec(approve_request_1).expect_success().commit();

    let account_1_balance_before = erc20_check_balance_of(&mut builder, owner);
    assert_eq!(account_1_balance_before, initial_supply);

    let account_1_allowance_before = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(account_1_allowance_before, allowance_amount_1);

    builder
        .exec(transfer_from_request_1)
        .expect_success()
        .commit();

    let account_1_allowance_after = erc20_check_allowance_of(&mut builder, owner, spender);
    assert_eq!(
        account_1_allowance_after,
        account_1_allowance_before - transfer_from_amount_1
    );

    let account_1_balance_after = erc20_check_balance_of(&mut builder, owner);
    assert_eq!(
        account_1_balance_after,
        account_1_balance_before - transfer_from_amount_1
    );
}
