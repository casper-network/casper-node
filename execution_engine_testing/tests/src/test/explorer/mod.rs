mod faucet;
mod faucet_deploy_builder;

// Test constants.
pub const FAUCET_INSTALLER_SESSION: &str = "faucet_stored.wasm";
pub const FAUCET_CONTRACT_NAMED_KEY: &str = "faucet";
pub const INSTALLER_FUND_AMOUNT: u64 = 500_000_000_000_000;
pub const TWO_HOURS_AS_MILLIS: u64 = 7_200_000;
pub const FAUCET_ID: u64 = 1337;
pub const INSTALLER_ACCOUNT: Lazy<AccountHash> = Lazy::new(|| AccountHash::new([1u8; 32]));
pub const FAUCET_FUND_AMOUNT: u64 = 500_000u64;
pub const FAUCET_DISTRIBUTIONS_PER_INTERVAL: u64 = 1;
pub const FAUCET_TIME_INTERVAL: u64 = 10_000u64;

// contract args and entry points.
pub const ARG_TARGET: &str = "target";
pub const ARG_AMOUNT: &str = "amount";
pub const ARG_ID: &str = "id";
pub const ARG_AVAILABLE_AMOUNT: &str = "available_amount";
pub const ARG_TIME_INTERVAL: &str = "time_interval";
pub const ARG_DISTRIBUTIONS_PER_INTERVAL: &str = "distributions_per_interval";
pub const ENTRY_POINT_FAUCET: &str = "call_faucet";
pub const ENTRY_POINT_SET_VARIABLES: &str = "set_variables";
pub const ENTRY_POINT_AUTHORIZE_TO: &str = "authorize_to";

// stored contract named keys.
pub const AVAILABLE_AMOUNT_NAMED_KEY: &str = "available_amount";
pub const TIME_INTERVAL_NAMED_KEY: &str = "time_interval";
pub const LAST_DISTRIBUTION_TIME_NAMED_KEY: &str = "last_distribution_time";
pub const FAUCET_PURSE_NAMED_KEY: &str = "faucet_purse";
pub const INSTALLER_NAMED_KEY: &str = "installer";
pub const DISTRIBUTIONS_PER_INTERVAL_NAMED_KEY: &str = "distributions_per_interval";
pub const REMAINING_REQUESTS_NAMED_KEY: &str = "remaining_requests";
pub const AUTHORIZED_ACCOUNT_NAMED_KEY: &str = "authorized_account";
