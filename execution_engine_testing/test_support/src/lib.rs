//! A library to support testing of Wasm smart contracts for use on the Casper Platform.

#![doc(html_root_url = "https://docs.rs/casper-engine-test-support/2.1.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]
mod additive_map_diff;
mod deploy_item_builder;
mod execute_request_builder;
mod step_request_builder;
mod upgrade_request_builder;
pub mod utils;
mod wasm_test_builder;

use num_rational::Ratio;
use once_cell::sync::Lazy;

use casper_execution_engine::{
    core::engine_state::{
        ChainspecRegistry, ExecConfig, GenesisAccount, GenesisConfig, RunGenesisRequest,
    },
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_hashing::Digest;
use casper_types::{account::AccountHash, Motes, ProtocolVersion, PublicKey, SecretKey, U512};

pub use additive_map_diff::AdditiveMapDiff;
pub use deploy_item_builder::DeployItemBuilder;
pub use execute_request_builder::ExecuteRequestBuilder;
pub use step_request_builder::StepRequestBuilder;
pub use upgrade_request_builder::UpgradeRequestBuilder;
pub use wasm_test_builder::{InMemoryWasmTestBuilder, LmdbWasmTestBuilder, WasmTestBuilder};

/// Default number of validator slots.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// Default auction delay.
pub const DEFAULT_AUCTION_DELAY: u64 = 3;
/// Default lock-in period of 90 days
pub const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 90 * 24 * 60 * 60 * 1000;
/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 14;

/// Default round seigniorage rate represented as a fractional number.
///
/// Annual issuance: 2%
/// Minimum round exponent: 14
/// Ticks per year: 31536000000
///
/// (1+0.02)^((2^14)/31536000000)-1 is expressed as a fraction below.
pub const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(6414, 623437335209);

/// Default chain name.
pub const DEFAULT_CHAIN_NAME: &str = "casper-execution-engine-testing";
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
/// Default maximum number of associated keys.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;
/// Default block time.
pub const DEFAULT_BLOCK_TIME: u64 = 0;
/// Default gas price.
pub const DEFAULT_GAS_PRICE: u64 = 1;
/// Amount named argument.
pub const ARG_AMOUNT: &str = "amount";
/// Timestamp increment in milliseconds.
pub const TIMESTAMP_MILLIS_INCREMENT: u64 = 30_000; // 30 seconds

/// Default genesis config hash.
pub static DEFAULT_GENESIS_CONFIG_HASH: Lazy<Digest> = Lazy::new(|| [42; 32].into());
/// Default account public key.
pub static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
/// Default test account address.
pub static DEFAULT_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
// NOTE: declaring DEFAULT_ACCOUNT_KEY as *DEFAULT_ACCOUNT_ADDR causes tests to stall.
/// Default account key.
pub static DEFAULT_ACCOUNT_KEY: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
/// Default initial balance of a test account in motes.
pub const DEFAULT_ACCOUNT_INITIAL_BALANCE: u64 = 100_000_000_000_000_000u64;
/// Minimal amount for a transfer that creates new accounts.
pub const MINIMUM_ACCOUNT_CREATION_BALANCE: u64 = 7_500_000_000_000_000u64;
/// Default proposer public key.
pub static DEFAULT_PROPOSER_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([198; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
/// Default proposer address.
pub static DEFAULT_PROPOSER_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_PROPOSER_PUBLIC_KEY));
/// Default accounts.
pub static DEFAULT_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut ret = Vec::new();
    let genesis_account = GenesisAccount::account(
        DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
        None,
    );
    ret.push(genesis_account);
    let proposer_account = GenesisAccount::account(
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
        None,
    );
    ret.push(proposer_account);
    ret
});
/// Default [`ProtocolVersion`].
pub static DEFAULT_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| ProtocolVersion::V1_0_0);
/// Default payment.
pub static DEFAULT_PAYMENT: Lazy<U512> = Lazy::new(|| U512::from(1_500_000_000_000u64));
/// Default [`WasmConfig`].
pub static DEFAULT_WASM_CONFIG: Lazy<WasmConfig> = Lazy::new(WasmConfig::default);
/// Default [`SystemConfig`].
pub static DEFAULT_SYSTEM_CONFIG: Lazy<SystemConfig> = Lazy::new(SystemConfig::default);
/// Default [`ExecConfig`].
pub static DEFAULT_EXEC_CONFIG: Lazy<ExecConfig> = Lazy::new(|| {
    ExecConfig::new(
        DEFAULT_ACCOUNTS.clone(),
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
/// Default [`GenesisConfig`].
pub static DEFAULT_GENESIS_CONFIG: Lazy<GenesisConfig> = Lazy::new(|| {
    GenesisConfig::new(
        DEFAULT_CHAIN_NAME.to_string(),
        DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        *DEFAULT_PROTOCOL_VERSION,
        DEFAULT_EXEC_CONFIG.clone(),
    )
});
/// Default [`ChainspecRegistry`].
pub static DEFAULT_CHAINSPEC_REGISTRY: Lazy<ChainspecRegistry> =
    Lazy::new(|| ChainspecRegistry::new_with_genesis(&[1, 2, 3], &[4, 5, 6]));
/// Default [`RunGenesisRequest`].
pub static DEFAULT_RUN_GENESIS_REQUEST: Lazy<RunGenesisRequest> = Lazy::new(|| {
    RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        DEFAULT_EXEC_CONFIG.clone(),
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    )
});
/// System address.
pub static SYSTEM_ADDR: Lazy<AccountHash> = Lazy::new(|| PublicKey::System.to_account_hash());
