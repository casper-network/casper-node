//! A library to support testing of Wasm smart contracts for use on the Casper Platform.

#![doc(html_root_url = "https://docs.rs/casper-engine-test-support/7.0.1")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon_48.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon.png",
    test(attr(deny(warnings)))
)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

mod chainspec_config;
mod deploy_item_builder;
mod execute_request_builder;
mod step_request_builder;
mod transfer_request_builder;
mod upgrade_request_builder;
pub mod utils;
mod wasm_test_builder;

use num_rational::Ratio;
use once_cell::sync::Lazy;

use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    account::AccountHash, testing::TestRng, ChainspecRegistry, Digest, GenesisAccount,
    GenesisConfig, GenesisConfigBuilder, Motes, ProtocolVersion, PublicKey, SecretKey,
    SystemConfig, WasmConfig, U512,
};

pub use chainspec_config::{ChainspecConfig, CHAINSPEC_SYMLINK};
pub use deploy_item_builder::DeployItemBuilder;
pub use execute_request_builder::{ExecuteRequest, ExecuteRequestBuilder};
pub use step_request_builder::StepRequestBuilder;
pub use transfer_request_builder::TransferRequestBuilder;
pub use upgrade_request_builder::UpgradeRequestBuilder;
pub use wasm_test_builder::{EntityWithNamedKeys, LmdbWasmTestBuilder, WasmTestBuilder};

/// Default number of validator slots.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// Default auction delay.
pub const DEFAULT_AUCTION_DELAY: u64 = 1;
/// Default lock-in period is currently zero.
pub const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 0;
/// Default length of total vesting schedule is currently zero.
pub const DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS: u64 = 0;

/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 7;

/// Round seigniorage rate represented as a fraction of the total supply.
///
/// Annual issuance: 8%
/// Minimum round length: 2^14 ms
/// Ticks per year: 31536000000
///
/// (1+0.08)^((2^14)/31536000000)-1 is expressed as a fractional number below.
pub const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(1, 4200000000000000000);

/// Default chain name.
pub const DEFAULT_CHAIN_NAME: &str = "casper-execution-engine-testing";
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
/// Default block time.
pub const DEFAULT_BLOCK_TIME: u64 = 0;
/// Default gas price.
pub const DEFAULT_GAS_PRICE: u8 = 1;
/// Amount named argument.
pub const ARG_AMOUNT: &str = "amount";
/// Timestamp increment in milliseconds.
pub const TIMESTAMP_MILLIS_INCREMENT: u64 = 30_000; // 30 seconds

/// Default value for maximum associated keys configuration option.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
/// Default value for maximum runtime call stack height configuration option.
pub const DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32 = 12;
/// Default value for minimum delegation amount in motes.
pub const DEFAULT_MINIMUM_DELEGATION_AMOUNT: u64 = 500 * 1_000_000_000;
/// Default genesis config hash.
pub const DEFAULT_GENESIS_CONFIG_HASH: Digest = Digest::from_raw([42; 32]);

/// Default account secret key.
pub static DEFAULT_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap());
/// Default account public key.
pub static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ACCOUNT_SECRET_KEY));
/// Default test account address.
pub static DEFAULT_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
// NOTE: declaring DEFAULT_ACCOUNT_KEY as *DEFAULT_ACCOUNT_ADDR causes tests to stall.
/// Default account key.
pub static DEFAULT_ACCOUNT_KEY: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
/// Default initial balance of a test account in motes.
pub const DEFAULT_ACCOUNT_INITIAL_BALANCE: u64 = 10_000_000_000_000_000_000_u64;
/// Minimal amount for a transfer that creates new accounts.
pub const MINIMUM_ACCOUNT_CREATION_BALANCE: u64 = 7_500_000_000_000_000_u64;
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
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        None,
    );
    ret.push(genesis_account);
    let proposer_account = GenesisAccount::account(
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        None,
    );
    ret.push(proposer_account);
    let rng = &mut TestRng::new();
    for _ in 0..10 {
        let filler_account = GenesisAccount::account(
            PublicKey::random(rng),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            None,
        );
        ret.push(filler_account);
    }
    ret
});
/// Default [`ProtocolVersion`].
pub const DEFAULT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V2_0_0;
/// Default payment.
pub static DEFAULT_PAYMENT: Lazy<U512> = Lazy::new(|| U512::from(2_500_000_000_000u64));
/// Default [`WasmConfig`].
pub static DEFAULT_WASM_CONFIG: Lazy<WasmConfig> = Lazy::new(WasmConfig::default);
/// Default [`SystemConfig`].
pub static DEFAULT_SYSTEM_CONFIG: Lazy<SystemConfig> = Lazy::new(SystemConfig::default);

/// Default [`GenesisConfig`].
pub static DEFAULT_EXEC_CONFIG: Lazy<GenesisConfig> = Lazy::new(|| {
    GenesisConfigBuilder::default()
        .with_accounts(DEFAULT_ACCOUNTS.clone())
        .with_wasm_config(*DEFAULT_WASM_CONFIG)
        .with_system_config(*DEFAULT_SYSTEM_CONFIG)
        .with_validator_slots(DEFAULT_VALIDATOR_SLOTS)
        .with_auction_delay(DEFAULT_AUCTION_DELAY)
        .with_locked_funds_period_millis(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS)
        .with_round_seigniorage_rate(DEFAULT_ROUND_SEIGNIORAGE_RATE)
        .with_unbonding_delay(DEFAULT_UNBONDING_DELAY)
        .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
        .build()
});

/// Default [`ChainspecRegistry`].
pub static DEFAULT_CHAINSPEC_REGISTRY: Lazy<ChainspecRegistry> =
    Lazy::new(|| ChainspecRegistry::new_with_genesis(&[1, 2, 3], &[4, 5, 6]));

/// A [`GenesisRequest`] using cost tables matching those used in Casper Mainnet.
pub static LOCAL_GENESIS_REQUEST: Lazy<GenesisRequest> = Lazy::new(|| {
    ChainspecConfig::create_genesis_request_from_local_chainspec(
        DEFAULT_ACCOUNTS.clone(),
        DEFAULT_PROTOCOL_VERSION,
    )
    .expect("must create the request")
});
/// Round seigniorage rate from the production chainspec.
pub static PRODUCTION_ROUND_SEIGNIORAGE_RATE: Lazy<Ratio<u64>> = Lazy::new(|| {
    let chainspec = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must create chainspec_config");
    chainspec.core_config.round_seigniorage_rate
});
/// System address.
pub static SYSTEM_ADDR: Lazy<AccountHash> = Lazy::new(|| PublicKey::System.to_account_hash());

#[cfg(test)]
mod tests {
    //use super::*;

    #[test]
    fn defaults_should_match_production_chainspec_values() {
        // TODO: Temporarily disabled. Now that the test_support uses its own chainspec, this check
        // may not be necessary.

        // let production = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK).unwrap();
        // // No need to test `CoreConfig::validator_slots`.
        // assert_eq!(production.core_config.auction_delay, DEFAULT_AUCTION_DELAY);
        // assert_eq!(
        //     production.core_config.locked_funds_period.millis(),
        //     DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS
        // );
        // assert_eq!(
        //     production.core_config.unbonding_delay,
        //     DEFAULT_UNBONDING_DELAY
        // );
        // assert_eq!(
        //     production.core_config.round_seigniorage_rate.reduced(),
        //     DEFAULT_ROUND_SEIGNIORAGE_RATE.reduced()
        // );
        // assert_eq!(
        //     production.core_config.max_associated_keys,
        //     DEFAULT_MAX_ASSOCIATED_KEYS
        // );
        // assert_eq!(
        //     production.core_config.max_runtime_call_stack_height,
        //     DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT
        // );
        // assert_eq!(
        //     production.core_config.minimum_delegation_amount,
        //     DEFAULT_MINIMUM_DELEGATION_AMOUNT
        // );

        // assert_eq!(production.wasm_config, WasmConfig::default());
        // TODO: reenable after new payment logic is added
        //assert_eq!(production.system_costs_config, SystemConfig::default());
    }
}
