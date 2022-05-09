mod fees_accumulation;
pub mod management;
mod restricted_auction;
mod unrestricted_transfers;
mod update_admins;

use std::collections::BTreeMap;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    DEFAULT_AUCTION_DELAY, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE,
    DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
    MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
};
use casper_execution_engine::core::engine_state::{
    engine_config::{EngineConfigBuilder, FeeElimination},
    genesis::{AdministratorAccount, GenesisValidator},
    EngineConfig, ExecConfig, GenesisAccount, RunGenesisRequest,
};
use once_cell::sync::Lazy;

use casper_types::{
    account::{AccountHash, Weight},
    runtime_args,
    system::{auction::DELEGATION_RATE_DENOMINATOR, mint},
    Motes, PublicKey, RuntimeArgs, SecretKey, U512,
};

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
const DEFAULT_ADMIN_ACCOUNT_WEIGHT: Weight = Weight::MAX;

static ADMIN_1_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([240; 32]).unwrap());
static ADMIN_1_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ADMIN_1_ACCOUNT_SECRET_KEY));
static ADMIN_1_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| ADMIN_1_ACCOUNT_PUBLIC_KEY.to_account_hash());
const ADMIN_1_ACCOUNT_WEIGHT: Weight = Weight::new(254);

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

const CONTROL_MANAGEMENT_CONTRACT: &str = "control_management.wasm";

const PRIVATE_CHAIN_ALLOW_AUCTION_BIDS: bool = false;
const PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS: bool = false;

static PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS: Lazy<Vec<AdministratorAccount>> = Lazy::new(|| {
    let default_admin = AdministratorAccount::new(
        DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(ADMIN_ACCOUNT_INITIAL_BALANCE),
        DEFAULT_ADMIN_ACCOUNT_WEIGHT,
    );
    let admin_1 = AdministratorAccount::new(
        ADMIN_1_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(ADMIN_ACCOUNT_INITIAL_BALANCE),
        ADMIN_1_ACCOUNT_WEIGHT,
    );
    vec![default_admin, admin_1]
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

const PRIVATE_CHAIN_FEE_ELIMINATION: FeeElimination = FeeElimination::Accumulate;

static DEFUALT_PRIVATE_CHAIN_EXEC_CONFIG: Lazy<ExecConfig> = Lazy::new(|| {
    ExecConfig::new(
        PRIVATE_CHAIN_DEFAULT_ACCOUNTS.clone(),
        *DEFAULT_WASM_CONFIG,
        *DEFAULT_SYSTEM_CONFIG,
        DEFAULT_VALIDATOR_SLOTS,
        DEFAULT_AUCTION_DELAY,
        DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        DEFAULT_ROUND_SEIGNIORAGE_RATE,
        DEFAULT_UNBONDING_DELAY,
        DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        PRIVATE_CHAIN_FEE_ELIMINATION,
    )
});

static DEFAULT_PRIVATE_CHAIN_GENESIS: Lazy<RunGenesisRequest> = Lazy::new(|| {
    RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        DEFUALT_PRIVATE_CHAIN_EXEC_CONFIG.clone(),
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    )
});

fn custom_private_chain_setup(
    allow_auction_bids: bool,
    allow_unrestricted_transfers: bool,
    fee_elimination: FeeElimination,
) -> InMemoryWasmTestBuilder {
    let mut builder = custom_setup_genesis_only(
        allow_auction_bids,
        allow_unrestricted_transfers,
        fee_elimination,
    );

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        CONTROL_MANAGEMENT_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    let transfer_args_1 = runtime_args! {
        mint::ARG_TARGET => *SYSTEM_ADDR,
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        mint::ARG_ID => <Option<u64>>::None,
    };

    let fund_system_request =
        ExecuteRequestBuilder::transfer(*DEFAULT_ADMIN_ACCOUNT_ADDR, transfer_args_1).build();

    builder.exec(exec_request).expect_success().commit();
    builder.exec(fund_system_request).expect_success().commit();

    builder
}

fn custom_setup_genesis_only(
    allow_auction_bids: bool,
    allow_unrestricted_transfers: bool,
    fee_elimination: FeeElimination,
) -> InMemoryWasmTestBuilder {
    let engine_config = make_engine_config(
        allow_auction_bids,
        allow_unrestricted_transfers,
        fee_elimination,
    );
    let mut builder = InMemoryWasmTestBuilder::new_with_config(engine_config);
    builder.run_genesis(&DEFAULT_PRIVATE_CHAIN_GENESIS);
    builder
}

fn setup_genesis_only() -> InMemoryWasmTestBuilder {
    custom_setup_genesis_only(
        PRIVATE_CHAIN_ALLOW_AUCTION_BIDS,
        PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS,
        PRIVATE_CHAIN_FEE_ELIMINATION,
    )
}

fn make_engine_config(
    allow_auction_bids: bool,
    allow_unrestricted_transfers: bool,
    fee_elimination: FeeElimination,
) -> EngineConfig {
    EngineConfigBuilder::default()
        .with_administrative_accounts(PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS.clone())
        .with_allow_auction_bids(allow_auction_bids)
        .with_allow_unrestricted_transfers(allow_unrestricted_transfers)
        .with_fee_elimination(fee_elimination)
        .build()
}

fn private_chain_setup() -> InMemoryWasmTestBuilder {
    custom_private_chain_setup(
        PRIVATE_CHAIN_ALLOW_AUCTION_BIDS,
        PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS,
        PRIVATE_CHAIN_FEE_ELIMINATION,
    )
}
