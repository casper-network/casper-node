use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ChainspecConfig, LmdbWasmTestBuilder, DEFAULT_AUCTION_DELAY, DEFAULT_CHAINSPEC_REGISTRY,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY,
    DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    account::AccountHash, addressable_entity::EntityKindTag, system::auction::DelegationRate,
    GenesisAccount, GenesisConfigBuilder, GenesisValidator, Key, Motes, ProtocolVersion, PublicKey,
    SecretKey, StoredValue, DEFAULT_FEE_HANDLING, DEFAULT_REFUND_HANDLING, U512,
};

const GENESIS_CONFIG_HASH: [u8; 32] = [127; 32];
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
    vec![account_1, account_2]
});

#[ignore]
#[test]
fn should_run_genesis() {
    let protocol_version = ProtocolVersion::V1_0_0;

    let run_genesis_request = ChainspecConfig::create_genesis_request_from_production_chainspec(
        GENESIS_CUSTOM_ACCOUNTS.clone(),
        protocol_version,
    )
    .expect("must create genesis request");

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let _system_account = builder
        .get_entity_by_account_hash(PublicKey::System.to_account_hash())
        .expect("system account should exist");

    let account_1 = builder
        .get_entity_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("account 1 should exist");

    let account_2 = builder
        .get_entity_by_account_hash(*ACCOUNT_2_ADDR)
        .expect("account 2 should exist");

    let account_1_balance_actual = builder.get_purse_balance(account_1.main_purse());
    let account_2_balance_actual = builder.get_purse_balance(account_2.main_purse());

    assert_eq!(account_1_balance_actual, U512::from(ACCOUNT_1_BALANCE));
    assert_eq!(account_2_balance_actual, U512::from(ACCOUNT_2_BALANCE));

    let mint_contract_key =
        Key::addressable_entity_key(EntityKindTag::System, builder.get_mint_contract_hash());
    let handle_payment_contract_key = Key::addressable_entity_key(
        EntityKindTag::System,
        builder.get_handle_payment_contract_hash(),
    );

    let result = builder.query(None, mint_contract_key, &[]);
    if let Ok(StoredValue::AddressableEntity(_)) = result {
        // Contract exists at mint contract hash
    } else {
        panic!("contract not found at mint hash");
    }

    if let Ok(StoredValue::AddressableEntity(_)) =
        builder.query(None, handle_payment_contract_key, &[])
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
    let wasm_config = *DEFAULT_WASM_CONFIG;
    let system_config = *DEFAULT_SYSTEM_CONFIG;
    let protocol_version = ProtocolVersion::V1_0_0;
    let validator_slots = DEFAULT_VALIDATOR_SLOTS;
    let auction_delay = DEFAULT_AUCTION_DELAY;
    let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;
    let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
    let unbonding_delay = DEFAULT_UNBONDING_DELAY;
    let genesis_timestamp = DEFAULT_GENESIS_TIMESTAMP_MILLIS;
    let refund_handling = DEFAULT_REFUND_HANDLING;
    let fee_handling = DEFAULT_FEE_HANDLING;
    let config = GenesisConfigBuilder::default()
        .with_accounts(accounts.clone())
        .with_wasm_config(wasm_config)
        .with_system_config(system_config)
        .with_validator_slots(validator_slots)
        .with_auction_delay(auction_delay)
        .with_locked_funds_period_millis(locked_funds_period)
        .with_round_seigniorage_rate(round_seigniorage_rate)
        .with_unbonding_delay(unbonding_delay)
        .with_genesis_timestamp_millis(genesis_timestamp)
        .with_refund_handling(refund_handling)
        .with_fee_handling(fee_handling)
        .build();

    let genesis_request = GenesisRequest::new(
        GENESIS_CONFIG_HASH.into(),
        protocol_version,
        config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(genesis_request);

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
