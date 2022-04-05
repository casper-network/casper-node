use crate::EraId;

use super::DelegationRate;

/// Initial value of era id we start at genesis.
pub const INITIAL_ERA_ID: EraId = EraId::new(0);

/// Initial value of era end timestamp.
pub const INITIAL_ERA_END_TIMESTAMP_MILLIS: u64 = 0;

/// Delegation rate is a fraction between 0-1. Validator sets the delegation rate
/// in integer terms, which is then divided by the denominator to obtain the fraction.
pub const DELEGATION_RATE_DENOMINATOR: DelegationRate = 100;

/// We use one trillion as a block reward unit because it's large enough to allow precise
/// fractions, and small enough for many block rewards to fit into a u64.
pub const BLOCK_REWARD: u64 = 1_000_000_000_000;

/// Named constant for `amount`.
pub const ARG_AMOUNT: &str = "amount";
/// Named constant for `delegation_rate`.
pub const ARG_DELEGATION_RATE: &str = "delegation_rate";
/// Named constant for `account_hash`.
pub const ARG_PUBLIC_KEY: &str = "public_key";
/// Named constant for `validator`.
pub const ARG_VALIDATOR: &str = "validator";
/// Named constant for `delegator`.
pub const ARG_DELEGATOR: &str = "delegator";
/// Named constant for `validator_purse`.
pub const ARG_VALIDATOR_PURSE: &str = "validator_purse";
/// Named constant for `validator_keys`.
pub const ARG_VALIDATOR_KEYS: &str = "validator_keys";
/// Named constant for `validator_public_keys`.
pub const ARG_VALIDATOR_PUBLIC_KEYS: &str = "validator_public_keys";
/// Named constant for `new_validator`.
pub const ARG_NEW_VALIDATOR: &str = "new_validator";
/// Named constant for `era_id`.
pub const ARG_ERA_ID: &str = "era_id";
/// Named constant for `reward_factors`.
pub const ARG_REWARD_FACTORS: &str = "reward_factors";
/// Named constant for `validator_public_key`.
pub const ARG_VALIDATOR_PUBLIC_KEY: &str = "validator_public_key";
/// Named constant for `delegator_public_key`.
pub const ARG_DELEGATOR_PUBLIC_KEY: &str = "delegator_public_key";
/// Named constant for `validator_slots` argument.
pub const ARG_VALIDATOR_SLOTS: &str = VALIDATOR_SLOTS_KEY;
/// Named constant for `mint_contract_package_hash`
pub const ARG_MINT_CONTRACT_PACKAGE_HASH: &str = "mint_contract_package_hash";
/// Named constant for `genesis_validators`
pub const ARG_GENESIS_VALIDATORS: &str = "genesis_validators";
/// Named constant of `auction_delay`
pub const ARG_AUCTION_DELAY: &str = "auction_delay";
/// Named constant for `locked_funds_period`
pub const ARG_LOCKED_FUNDS_PERIOD: &str = "locked_funds_period";
/// Named constant for `unbonding_delay`
pub const ARG_UNBONDING_DELAY: &str = "unbonding_delay";
/// Named constant for `era_end_timestamp_millis`;
pub const ARG_ERA_END_TIMESTAMP_MILLIS: &str = "era_end_timestamp_millis";
/// Named constant for `evicted_validators`;
pub const ARG_EVICTED_VALIDATORS: &str = "evicted_validators";

/// Named constant for method `get_era_validators`.
pub const METHOD_GET_ERA_VALIDATORS: &str = "get_era_validators";
/// Named constant for method `add_bid`.
pub const METHOD_ADD_BID: &str = "add_bid";
/// Named constant for method `withdraw_bid`.
pub const METHOD_WITHDRAW_BID: &str = "withdraw_bid";
/// Named constant for method `delegate`.
pub const METHOD_DELEGATE: &str = "delegate";
/// Named constant for method `undelegate`.
pub const METHOD_UNDELEGATE: &str = "undelegate";
/// Named constant for method `redelegate`.
pub const METHOD_REDELEGATE: &str = "redelegate";
/// Named constant for method `run_auction`.
pub const METHOD_RUN_AUCTION: &str = "run_auction";
/// Named constant for method `slash`.
pub const METHOD_SLASH: &str = "slash";
/// Named constant for method `distribute`.
pub const METHOD_DISTRIBUTE: &str = "distribute";
/// Named constant for method `read_era_id`.
pub const METHOD_READ_ERA_ID: &str = "read_era_id";
/// Named constant for method `activate_bid`.
pub const METHOD_ACTIVATE_BID: &str = "activate_bid";

/// Storage for `EraId`.
pub const ERA_ID_KEY: &str = "era_id";
/// Storage for era-end timestamp.
pub const ERA_END_TIMESTAMP_MILLIS_KEY: &str = "era_end_timestamp_millis";
/// Storage for `SeigniorageRecipientsSnapshot`.
pub const SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY: &str = "seigniorage_recipients_snapshot";
/// Total validator slots allowed.
pub const VALIDATOR_SLOTS_KEY: &str = "validator_slots";
/// Amount of auction delay.
pub const AUCTION_DELAY_KEY: &str = "auction_delay";
/// Default lock period for new bid entries represented in eras.
pub const LOCKED_FUNDS_PERIOD_KEY: &str = "locked_funds_period";
/// Unbonding delay expressed in eras.
pub const UNBONDING_DELAY_KEY: &str = "unbonding_delay";
