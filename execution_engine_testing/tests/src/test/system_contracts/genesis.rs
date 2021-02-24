use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{
        InMemoryWasmTestBuilder, DEFAULT_AUCTION_DELAY, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_SYSTEM_CONFIG,
        DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
    },
    AccountHash,
};
use casper_execution_engine::{
    core::engine_state::{
        genesis::{ExecConfig, GenesisAccount},
        run_genesis_request::RunGenesisRequest,
        SYSTEM_ACCOUNT_ADDR,
    },
    shared::{motes::Motes, stored_value::StoredValue},
};
use casper_types::{ProtocolVersion, PublicKey, SecretKey, U512};

const GENESIS_CONFIG_HASH: [u8; 32] = [127; 32];
const ACCOUNT_1_BONDED_AMOUNT: u64 = 1_000_000;
const ACCOUNT_2_BONDED_AMOUNT: u64 = 2_000_000;
const ACCOUNT_1_BALANCE: u64 = 1_000_000_000;
const ACCOUNT_2_BALANCE: u64 = 2_000_000_000;

static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([42; SecretKey::ED25519_LENGTH]).into());
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PUBLIC_KEY));
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([44; SecretKey::ED25519_LENGTH]).into());
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_2_PUBLIC_KEY));

static GENESIS_CUSTOM_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let account_1 = {
        let account_1_balance = Motes::new(ACCOUNT_1_BALANCE.into());
        let account_1_bonded_amount = Motes::new(ACCOUNT_1_BONDED_AMOUNT.into());
        GenesisAccount::account(
            *ACCOUNT_1_PUBLIC_KEY,
            account_1_balance,
            account_1_bonded_amount,
        )
    };
    let account_2 = {
        let account_2_balance = Motes::new(ACCOUNT_2_BALANCE.into());
        let account_2_bonded_amount = Motes::new(ACCOUNT_2_BONDED_AMOUNT.into());
        GenesisAccount::account(
            *ACCOUNT_2_PUBLIC_KEY,
            account_2_balance,
            account_2_bonded_amount,
        )
    };
    vec![account_1, account_2]
});

#[ignore]
#[test]
fn should_run_genesis() {
    let protocol_version = ProtocolVersion::V1_0_0;
    let wasm_config = *DEFAULT_WASM_CONFIG;
    let system_config = *DEFAULT_SYSTEM_CONFIG;
    let validator_slots = DEFAULT_VALIDATOR_SLOTS;
    let auction_delay = DEFAULT_AUCTION_DELAY;
    let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;
    let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
    let unbonding_delay = DEFAULT_UNBONDING_DELAY;
    let genesis_timestamp = DEFAULT_GENESIS_TIMESTAMP_MILLIS;

    let exec_config = ExecConfig::new(
        GENESIS_CUSTOM_ACCOUNTS.clone(),
        wasm_config,
        system_config,
        validator_slots,
        auction_delay,
        locked_funds_period,
        round_seigniorage_rate,
        unbonding_delay,
        genesis_timestamp,
    );
    let run_genesis_request =
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, exec_config);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let system_account = builder
        .get_account(SYSTEM_ACCOUNT_ADDR)
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
    let pos_contract_hash = builder.get_pos_contract_hash();

    let result = builder.query(None, mint_contract_hash.into(), &[]);
    if let Ok(StoredValue::Contract(_)) = result {
        // Contract exists at mint contract hash
    } else {
        panic!("contract not found at mint hash");
    }

    if let Ok(StoredValue::Contract(_)) = builder.query(None, pos_contract_hash.into(), &[]) {
        // Contract exists at pos contract hash
    } else {
        panic!("contract not found at pos hash");
    }
}

#[ignore]
#[test]
fn should_track_total_token_supply_in_mint() {
    let accounts = GENESIS_CUSTOM_ACCOUNTS.clone();
    let wasm_config = *DEFAULT_WASM_CONFIG;
    let system_config = *DEFAULT_SYSTEM_CONFIG;
    let protocol_version = ProtocolVersion::V1_0_0;
    let validator_slots = DEFAULT_VALIDATOR_SLOTS;
    let auction_delay = DEFAULT_AUCTION_DELAY;
    let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;
    let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
    let unbonding_delay = DEFAULT_UNBONDING_DELAY;
    let genesis_tiemstamp = DEFAULT_GENESIS_TIMESTAMP_MILLIS;
    let ee_config = ExecConfig::new(
        accounts.clone(),
        wasm_config,
        system_config,
        validator_slots,
        auction_delay,
        locked_funds_period,
        round_seigniorage_rate,
        unbonding_delay,
        genesis_tiemstamp,
    );
    let run_genesis_request =
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, ee_config);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

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
