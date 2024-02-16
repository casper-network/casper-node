mod burn_fees_and_refund;
mod fees_accumulation;
pub mod management;
mod restricted_auction;
mod unrestricted_transfers;

use std::collections::{BTreeMap, BTreeSet};

use casper_engine_test_support::{
    LmdbWasmTestBuilder, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_AUCTION_DELAY,
    DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION,
    DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY,
    DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
};
use casper_execution_engine::engine_state::{EngineConfig, EngineConfigBuilder};
use num_rational::Ratio;
use once_cell::sync::Lazy;

use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    account::AccountHash, system::auction::DELEGATION_RATE_DENOMINATOR, AdministratorAccount,
    FeeHandling, GenesisAccount, GenesisConfig, GenesisConfigBuilder, GenesisValidator,
    HostFunction, HostFunctionCosts, MessageLimits, Motes, OpcodeCosts, PublicKey, RefundHandling,
    SecretKey, StorageCosts, WasmConfig, DEFAULT_MAX_STACK_HEIGHT, DEFAULT_WASM_MAX_MEMORY, U512,
};
use tempfile::TempDir;

static VALIDATOR_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([244; 32]).unwrap());
static VALIDATOR_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*VALIDATOR_1_SECRET_KEY));

const DEFAULT_VALIDATOR_BONDED_AMOUNT: U512 = U512([u64::MAX, 0, 0, 0, 0, 0, 0, 0]);

static DEFAULT_ADMIN_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([250; 32]).unwrap());
static DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ADMIN_ACCOUNT_SECRET_KEY));
static DEFAULT_ADMIN_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY.to_account_hash());

static ADMIN_1_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([240; 32]).unwrap());
static ADMIN_1_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ADMIN_1_ACCOUNT_SECRET_KEY));
static ADMIN_1_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| ADMIN_1_ACCOUNT_PUBLIC_KEY.to_account_hash());

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([251; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_1_PUBLIC_KEY.to_account_hash());

static ACCOUNT_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([241; 32]).unwrap());
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_2_SECRET_KEY));
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_2_PUBLIC_KEY.to_account_hash());

const ADMIN_ACCOUNT_INITIAL_BALANCE: U512 = U512([100_000_000_000_000_000u64, 0, 0, 0, 0, 0, 0, 0]);

const PRIVATE_CHAIN_ALLOW_AUCTION_BIDS: bool = false;
const PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS: bool = false;

static PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS: Lazy<Vec<AdministratorAccount>> = Lazy::new(|| {
    let default_admin = AdministratorAccount::new(
        DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(ADMIN_ACCOUNT_INITIAL_BALANCE),
    );
    let admin_1 = AdministratorAccount::new(
        ADMIN_1_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(ADMIN_ACCOUNT_INITIAL_BALANCE),
    );
    vec![default_admin, admin_1]
});

static PRIVATE_CHAIN_GENESIS_ADMIN_SET: Lazy<BTreeSet<PublicKey>> = Lazy::new(|| {
    PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS
        .iter()
        .map(|admin| admin.public_key().clone())
        .collect()
});

static PRIVATE_CHAIN_GENESIS_VALIDATORS: Lazy<BTreeMap<PublicKey, GenesisValidator>> =
    Lazy::new(|| {
        let public_key = VALIDATOR_1_PUBLIC_KEY.clone();
        let genesis_validator_1 = GenesisValidator::new(
            Motes::new(DEFAULT_VALIDATOR_BONDED_AMOUNT),
            DELEGATION_RATE_DENOMINATOR,
        );
        let mut genesis_validators = BTreeMap::new();
        genesis_validators.insert(public_key, genesis_validator_1);
        genesis_validators
    });

static PRIVATE_CHAIN_DEFAULT_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut default_accounts = Vec::new();

    let proposer_account = GenesisAccount::account(
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        Motes::new(U512::zero()),
        None,
    );
    default_accounts.push(proposer_account);

    // One normal account that starts at genesis
    default_accounts.push(GenesisAccount::account(
        ACCOUNT_1_PUBLIC_KEY.clone(),
        Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
        None,
    ));

    // Set up genesis validators
    {
        let public_key = VALIDATOR_1_PUBLIC_KEY.clone();
        let genesis_validator = PRIVATE_CHAIN_GENESIS_VALIDATORS[&public_key];
        default_accounts.push(GenesisAccount::Account {
            public_key,
            // Genesis validators for a private network doesn't have balances, but they are part of
            // fixed set of validators
            balance: Motes::new(U512::zero()),
            validator: Some(genesis_validator),
        });
    }

    let admin_accounts = PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS.clone();
    let genesis_admins = admin_accounts.into_iter().map(GenesisAccount::from);
    default_accounts.extend(genesis_admins);

    default_accounts
});

const PRIVATE_CHAIN_REFUND_HANDLING: RefundHandling = RefundHandling::Refund {
    refund_ratio: Ratio::new_raw(1, 1),
};
const PRIVATE_CHAIN_FEE_HANDLING: FeeHandling = FeeHandling::Accumulate;
const PRIVATE_CHAIN_COMPUTE_REWARDS: bool = false;

static DEFUALT_PRIVATE_CHAIN_EXEC_CONFIG: Lazy<GenesisConfig> = Lazy::new(|| {
    GenesisConfigBuilder::default()
        .with_accounts(PRIVATE_CHAIN_DEFAULT_ACCOUNTS.clone())
        .with_wasm_config(*DEFAULT_WASM_CONFIG)
        .with_system_config(*DEFAULT_SYSTEM_CONFIG)
        .with_validator_slots(DEFAULT_VALIDATOR_SLOTS)
        .with_auction_delay(DEFAULT_AUCTION_DELAY)
        .with_locked_funds_period_millis(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS)
        .with_round_seigniorage_rate(DEFAULT_ROUND_SEIGNIORAGE_RATE)
        .with_unbonding_delay(DEFAULT_UNBONDING_DELAY)
        .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
        .with_refund_handling(PRIVATE_CHAIN_REFUND_HANDLING)
        .with_fee_handling(PRIVATE_CHAIN_FEE_HANDLING)
        .build()
});

static DEFAULT_PRIVATE_CHAIN_GENESIS: Lazy<GenesisRequest> = Lazy::new(|| {
    GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        DEFUALT_PRIVATE_CHAIN_EXEC_CONFIG.clone(),
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    )
});

fn custom_setup_genesis_only(
    allow_auction_bids: bool,
    allow_unrestricted_transfers: bool,
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
    compute_rewards: bool,
) -> LmdbWasmTestBuilder {
    let engine_config = make_engine_config(
        allow_auction_bids,
        allow_unrestricted_transfers,
        refund_handling,
        fee_handling,
        compute_rewards,
    );
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.as_ref(), engine_config);
    builder.run_genesis(DEFAULT_PRIVATE_CHAIN_GENESIS.clone());
    builder
}

fn setup_genesis_only() -> LmdbWasmTestBuilder {
    custom_setup_genesis_only(
        PRIVATE_CHAIN_ALLOW_AUCTION_BIDS,
        PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS,
        PRIVATE_CHAIN_REFUND_HANDLING,
        PRIVATE_CHAIN_FEE_HANDLING,
        PRIVATE_CHAIN_COMPUTE_REWARDS,
    )
}

fn make_wasm_config() -> WasmConfig {
    let host_functions = HostFunctionCosts {
        // Required for non-standard payment that transfers to a system account.
        // Depends on a bug filled to lower transfer host functions to be able to freely transfer
        // funds inside payment code.
        transfer_from_purse_to_account: HostFunction::fixed(0),
        ..HostFunctionCosts::default()
    };
    WasmConfig::new(
        DEFAULT_WASM_MAX_MEMORY,
        DEFAULT_MAX_STACK_HEIGHT,
        OpcodeCosts::default(),
        StorageCosts::default(),
        host_functions,
        MessageLimits::default(),
    )
}

fn make_engine_config(
    allow_auction_bids: bool,
    allow_unrestricted_transfers: bool,
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
    compute_rewards: bool,
) -> EngineConfig {
    let administrator_accounts = PRIVATE_CHAIN_GENESIS_ADMIN_SET.clone();
    EngineConfigBuilder::default()
        .with_administrative_accounts(administrator_accounts)
        .with_allow_auction_bids(allow_auction_bids)
        .with_allow_unrestricted_transfers(allow_unrestricted_transfers)
        .with_refund_handling(refund_handling)
        .with_fee_handling(fee_handling)
        .with_wasm_config(make_wasm_config())
        .with_compute_rewards(compute_rewards)
        .build()
}

fn private_chain_setup() -> LmdbWasmTestBuilder {
    custom_setup_genesis_only(
        PRIVATE_CHAIN_ALLOW_AUCTION_BIDS,
        PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS,
        PRIVATE_CHAIN_REFUND_HANDLING,
        PRIVATE_CHAIN_FEE_HANDLING,
        PRIVATE_CHAIN_COMPUTE_REWARDS,
    )
}
