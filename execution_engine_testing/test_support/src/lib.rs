//! A library to support testing of Wasm smart contracts for use on the Casper Platform.
//!
//! # Example
//! Consider a contract held in "contract.wasm" which stores an arbitrary `String` under a `Key`
//! named "special_value":
//! ```no_run
//! use casper_contract::contract_api::{runtime, storage};
//! use casper_types::Key;
//! const KEY: &str = "special_value";
//! const ARG_VALUE: &str = "value";
//!
//! #[no_mangle]
//! pub extern "C" fn call() {
//!     let value: String = runtime::get_named_arg(ARG_VALUE);
//!     let value_ref = storage::new_uref(value);
//!     let value_key: Key = value_ref.into();
//!     runtime::put_key(KEY, value_key);
//! }
//! ```
//!
//! The test could be written as follows:
//! ```no_run
//! use casper_engine_test_support::{Code, Error, SessionBuilder, TestContextBuilder, Value};
//! use casper_types::{U512, RuntimeArgs, runtime_args, PublicKey, account::AccountHash, SecretKey};
//!
//!
//! const MY_ACCOUNT: [u8; 32] = [7u8; 32];
//! const MY_ADDR: [u8; 32] = [8u8; 32];
//! const KEY: &str = "special_value";
//! const VALUE: &str = "hello world";
//! const ARG_MESSAGE: &str = "message";
//!
//! let secret_key = SecretKey::ed25519_from_bytes(MY_ACCOUNT).unwrap();
//! let public_key = PublicKey::from(&secret_key);
//! let account_addr = AccountHash::new(MY_ADDR);
//!
//! let mut context = TestContextBuilder::new()
//!     .with_public_key(public_key, U512::from(128_000_000_000_000u64))
//!     .build();
//!
//! // The test framework checks for compiled Wasm files in '<current working dir>/wasm'.  Paths
//! // relative to the current working dir (e.g. 'wasm/contract.wasm') can also be used, as can
//! // absolute paths.
//! let session_code = Code::from("contract.wasm");
//! let session_args = runtime_args! {
//!     ARG_MESSAGE => VALUE,
//! };
//! let session = SessionBuilder::new(session_code, session_args)
//!     .with_address(account_addr)
//!     .with_authorization_keys(&[account_addr])
//!     .build();
//!
//! let result_of_query: Result<Value, Error> = context.run(session).query(account_addr, &[KEY.to_string()]);
//!
//! let returned_value = result_of_query.expect("should be a value");
//!
//! let expected_value = Value::from_t(VALUE.to_string()).expect("should construct Value");
//! assert_eq!(expected_value, returned_value);
//! ```

#![doc(html_root_url = "https://docs.rs/casper-engine-test-support/1.0.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]

mod account;
mod additive_map_diff;
mod code;
mod deploy_item_builder;
mod error;
mod execute_request_builder;
mod session;
mod step_request_builder;
mod test_context;
mod upgrade_request_builder;
pub mod utils;
mod value;
mod wasm_test_builder;

/// TODO: doc comment.
pub use account::Account;
/// TODO: doc comment.
pub use casper_types::account::AccountHash;
/// TODO: doc comment.
pub use code::Code;
/// TODO: doc comment.
pub use error::{Error, Result};
/// TODO: doc comment.
pub use session::{Session, SessionBuilder, SessionTransferInfo};
/// TODO: doc comment.
pub use test_context::{TestContext, TestContextBuilder};
/// TODO: doc comment.
pub use value::Value;

/// The address of a [`URef`](casper_types::URef) (unforgeable reference) on the network.
pub type URefAddr = [u8; 32];

/// The hash of a smart contract stored on the network, which can be used to reference the contract.
pub type Hash = [u8; 32];

/// Default initial balance of a test account in motes.
pub const DEFAULT_ACCOUNT_INITIAL_BALANCE: u64 = 100_000_000_000_000_000u64;

/// Minimal amount for a transfer that creates new accounts.
pub const MINIMUM_ACCOUNT_CREATION_BALANCE: u64 = 7_500_000_000_000_000u64;

use num_rational::Ratio;
use once_cell::sync::Lazy;

use casper_execution_engine::{
    core::engine_state::{
        genesis::{ExecConfig, GenesisAccount, GenesisConfig},
        run_genesis_request::RunGenesisRequest,
    },
    shared::{newtypes::Blake2bHash, system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_types::{Motes, ProtocolVersion, PublicKey, SecretKey, U512};

pub use additive_map_diff::AdditiveMapDiff;
pub use deploy_item_builder::DeployItemBuilder;
pub use execute_request_builder::ExecuteRequestBuilder;
pub use step_request_builder::StepRequestBuilder;
pub use upgrade_request_builder::UpgradeRequestBuilder;
pub use wasm_test_builder::{
    InMemoryWasmTestContext, LmdbWasmTestContext, WasmTestContext, WasmTestResult,
};

/// TODO: doc comment.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// TODO: doc comment.
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

/// TODO: doc comment.
pub const DEFAULT_CHAIN_NAME: &str = "gerald";
/// TODO: doc comment.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
/// TODO: doc comment.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;
/// TODO: doc comment.
pub const DEFAULT_BLOCK_TIME: u64 = 0;
/// TODO: doc comment.
pub const DEFAULT_GAS_PRICE: u64 = 1;
/// TODO: doc comment.
pub const MOCKED_ACCOUNT_ADDRESS: AccountHash = AccountHash::new([48u8; 32]);
/// TODO: doc comment.
pub const ARG_AMOUNT: &str = "amount";
/// TODO: doc comment.
pub const TIMESTAMP_MILLIS_INCREMENT: u64 = 30000; // 30 seconds

// NOTE: Those values could be constants but are kept as once_cell::sync::Lazy to avoid changes of
// `*FOO` into `FOO` back and forth.
/// TODO: doc comment.
pub static DEFAULT_GENESIS_CONFIG_HASH: Lazy<Blake2bHash> = Lazy::new(|| [42; 32].into());
/// TODO: doc comment.
pub static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
/// Default test account address.
pub static DEFAULT_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
// Declaring DEFAULT_ACCOUNT_KEY as *DEFAULT_ACCOUNT_ADDR causes tests to stall.
/// TODO: doc comment.
pub static DEFAULT_ACCOUNT_KEY: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
/// TODO: doc comment.
pub static DEFAULT_PROPOSER_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([198; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
/// TODO: doc comment.
pub static DEFAULT_PROPOSER_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_PROPOSER_PUBLIC_KEY));
/// TODO: doc comment.
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
/// TODO: doc comment.
pub static DEFAULT_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| ProtocolVersion::V1_0_0);
/// TODO: doc comment.
pub static DEFAULT_PAYMENT: Lazy<U512> = Lazy::new(|| U512::from(1_500_000_000_000u64));
/// TODO: doc comment.
pub static DEFAULT_WASM_CONFIG: Lazy<WasmConfig> = Lazy::new(WasmConfig::default);
/// TODO: doc comment.
pub static DEFAULT_SYSTEM_CONFIG: Lazy<SystemConfig> = Lazy::new(SystemConfig::default);
/// TODO: doc comment.
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
/// TODO: doc comment.
pub static DEFAULT_GENESIS_CONFIG: Lazy<GenesisConfig> = Lazy::new(|| {
    GenesisConfig::new(
        DEFAULT_CHAIN_NAME.to_string(),
        DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        *DEFAULT_PROTOCOL_VERSION,
        DEFAULT_EXEC_CONFIG.clone(),
    )
});
/// TODO: doc comment.
pub static DEFAULT_RUN_GENESIS_REQUEST: Lazy<RunGenesisRequest> = Lazy::new(|| {
    RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        DEFAULT_EXEC_CONFIG.clone(),
    )
});
/// TODO: doc comment.
pub static SYSTEM_ADDR: Lazy<AccountHash> = Lazy::new(|| PublicKey::System.to_account_hash());
