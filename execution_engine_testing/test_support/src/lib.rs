//! A library to support testing of Wasm smart contracts for use on the Casper Platform.

#![doc(html_root_url = "https://docs.rs/casper-engine-test-support/2.0.3")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]
mod additive_map_diff;
mod chainspec_config;
mod deploy_item_builder;
mod execute_request_builder;
mod step_request_builder;
mod upgrade_request_builder;
pub mod utils;
mod wasm_test_builder;

use once_cell::sync::Lazy;

use casper_execution_engine::core::engine_state::genesis::GenesisAccount;
use casper_hashing::Digest;
use casper_types::{account::AccountHash, Motes, ProtocolVersion, PublicKey, SecretKey, U512};

pub use additive_map_diff::AdditiveMapDiff;
pub use chainspec_config::{ChainspecConfig, PRODUCTION_PATH};
pub use deploy_item_builder::DeployItemBuilder;
pub use execute_request_builder::ExecuteRequestBuilder;
pub use step_request_builder::StepRequestBuilder;
pub use upgrade_request_builder::UpgradeRequestBuilder;
pub use wasm_test_builder::{InMemoryWasmTestBuilder, LmdbWasmTestBuilder, WasmTestBuilder};

/// Default chain name.
pub const DEFAULT_CHAIN_NAME: &str = "casper-execution-engine-testing";
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
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
/// System address.
pub static SYSTEM_ADDR: Lazy<AccountHash> = Lazy::new(|| PublicKey::System.to_account_hash());
