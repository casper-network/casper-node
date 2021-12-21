use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
};
use casper_execution_engine::core::engine_state::genesis::{GenesisAccount, GenesisValidator};
use casper_types::{
    account::AccountHash, system::auction::DelegationRate, Motes, PublicKey, SecretKey,
    StoredValue, U512,
};

const ACCOUNT_1_BONDED_AMOUNT: u64 = 1_000_000;
const ACCOUNT_2_BONDED_AMOUNT: u64 = 2_000_000;
const ACCOUNT_1_BALANCE: u64 = 1_000_000_000;
const ACCOUNT_2_BALANCE: u64 = 2_000_000_000;

static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PUBLIC_KEY));
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_2_PUBLIC_KEY));

static GENESIS_CUSTOM_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut default_accounts = DEFAULT_ACCOUNTS.clone();
    let account_1 = {
        let account_1_balance = Motes::new(ACCOUNT_1_BALANCE.into());
        let account_1_bonded_amount = Motes::new(ACCOUNT_1_BONDED_AMOUNT.into());
        GenesisAccount::account(
            ACCOUNT_1_PUBLIC_KEY.clone(),
            account_1_balance,
            Some(GenesisValidator::new(
                account_1_bonded_amount,
                DelegationRate::zero(),
            )),
        )
    };
    default_accounts.push(account_1);
    let account_2 = {
        let account_2_balance = Motes::new(ACCOUNT_2_BALANCE.into());
        let account_2_bonded_amount = Motes::new(ACCOUNT_2_BONDED_AMOUNT.into());
        GenesisAccount::account(
            ACCOUNT_2_PUBLIC_KEY.clone(),
            account_2_balance,
            Some(GenesisValidator::new(
                account_2_bonded_amount,
                DelegationRate::zero(),
            )),
        )
    };
    default_accounts.push(account_2);
    default_accounts
});

#[ignore]
#[test]
fn should_run_genesis() {
    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder.run_genesis_with_custom_genesis_accounts(GENESIS_CUSTOM_ACCOUNTS.clone());

    let system_account = builder
        .get_account(PublicKey::System.to_account_hash())
        .expect("system account should exist");

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("account 1 should exist");

    let account_2 = builder
        .get_account(*ACCOUNT_2_ADDR)
        .expect("account 2 should exist");

    let system_account_balance_actual = builder.get_purse_balance(system_account.main_purse());
    let account_1_balance_actual = builder.get_purse_balance(account_1.main_purse());
    let account_2_balance_actual = builder.get_purse_balance(account_2.main_purse());

    assert_eq!(system_account_balance_actual, U512::zero());
    assert_eq!(account_1_balance_actual, U512::from(ACCOUNT_1_BALANCE));
    assert_eq!(account_2_balance_actual, U512::from(ACCOUNT_2_BALANCE));

    let mint_contract_hash = builder.get_mint_contract_hash();
    let handle_payment_contract_hash = builder.get_handle_payment_contract_hash();

    let result = builder.query(None, mint_contract_hash.into(), &[]);
    if let Ok(StoredValue::Contract(_)) = result {
        // Contract exists at mint contract hash
    } else {
        panic!("contract not found at mint hash");
    }

    if let Ok(StoredValue::Contract(_)) =
        builder.query(None, handle_payment_contract_hash.into(), &[])
    {
        // Contract exists at handle payment contract hash
    } else {
        panic!("contract not found at handle payment hash");
    }
}

#[ignore]
#[test]
fn should_track_total_token_supply_in_mint() {
    let accounts = GENESIS_CUSTOM_ACCOUNTS.clone();
    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder.run_genesis_with_custom_genesis_accounts(accounts.clone());

    let total_supply = builder.total_supply(None);

    let expected_balance: U512 = accounts.iter().map(|item| item.balance().value()).sum();
    let expected_staked_amount: U512 = accounts
        .iter()
        .map(|item| item.staked_amount().value())
        .sum();

    // check total supply against expected
    assert_eq!(
        total_supply,
        expected_balance + expected_staked_amount,
        "unexpected total supply"
    )
}

#[ignore]
#[test]
fn should_maybe_work() {
    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);
    builder.run_genesis_with_default_genesis_accounts();
    let _account_1 = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("default account should exist");
}
