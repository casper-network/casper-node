pub mod management;

use casper_engine_test_support::{
    DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_AUCTION_DELAY, DEFAULT_CHAINSPEC_REGISTRY,
    DEFAULT_GENESIS_CONFIG_HASH, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION,
    DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY,
    DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
};
use casper_execution_engine::core::engine_state::{
    genesis::AdministratorAccount, ExecConfig, GenesisAccount, RunGenesisRequest,
};
use once_cell::sync::Lazy;

use casper_types::{account::AccountHash, Motes, PublicKey, SecretKey, U512};

static DEFAULT_ADMIN_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([250; 32]).unwrap());
static DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ADMIN_ACCOUNT_SECRET_KEY));
static DEFAULT_ADMIN_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY.to_account_hash());

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

const SPECIAL_ACCOUNT_INITIAL_BALANCE: U512 =
    U512([100_000_000_000_000_000u64, 0, 0, 0, 0, 0, 0, 0]);

static PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS: Lazy<Vec<AdministratorAccount>> = Lazy::new(|| {
    let admin_account = AdministratorAccount::new(
        DEFAULT_ADMIN_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(SPECIAL_ACCOUNT_INITIAL_BALANCE),
    );
    vec![admin_account]
});

static PRIVATE_CHAIN_DEFAULT_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut default_accounts = Vec::new();

    let proposer_account = GenesisAccount::account(
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        // Default proposer account does not have balance as only the special account can hold
        // tokens on private chains.
        Motes::new(U512::zero()),
        None,
    );
    default_accounts.push(proposer_account);

    default_accounts.push(GenesisAccount::account(
        ACCOUNT_1_PUBLIC_KEY.clone(),
        Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
        None,
    ));

    let admin_accounts = PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS.clone();
    let genesis_admins = admin_accounts.into_iter().map(GenesisAccount::from);
    default_accounts.extend(genesis_admins);

    default_accounts
});

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
