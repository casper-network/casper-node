use super::EraId;
use crate::account::AccountHash;

/// System account hash.
pub const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0; 32]);

/// Maximum number of era validator slots.
pub const AUCTION_SLOTS: usize = 5;

/// Number of eras before an auction actually defines the set of validators.
pub const AUCTION_DELAY: u64 = 3;

/// Number of eras to keep track of in past.
pub const SNAPSHOT_SIZE: usize = AUCTION_DELAY as usize + 1;

/// Initial value of era id we start at genesis.
pub const INITIAL_ERA_ID: EraId = 0;

/// Default lock period for new bid entries represented in eras.
pub const DEFAULT_LOCKED_FUNDS_PERIOD: EraId = 15;

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
/// Named constant for `source_purse`.
pub const ARG_SOURCE_PURSE: &str = "source_purse";
/// Named constant for `validator_purse`.
pub const ARG_VALIDATOR_PURSE: &str = "validator_purse";
/// Named constant for `validator_keys`.
pub const ARG_VALIDATOR_KEYS: &str = "validator_keys";
/// Named constant for `validator_public_keys`.
pub const ARG_VALIDATOR_PUBLIC_KEYS: &str = "validator_public_keys";
/// Named constant for `era_id`.
pub const ARG_ERA_ID: &str = "era_id";
/// Named constant for `reward_factors`.
pub const ARG_REWARD_FACTORS: &str = "reward_factors";

/// Named constant for method `get_era_validators`.
pub const METHOD_GET_ERA_VALIDATORS: &str = "get_era_validators";
/// Named constant for method `read_seigniorage_recipients`.
pub const METHOD_READ_SEIGNIORAGE_RECIPIENTS: &str = "read_seigniorage_recipients";
/// Named constant for method `add_bid`.
pub const METHOD_ADD_BID: &str = "add_bid";
/// Named constant for method `withdraw_bid`.
pub const METHOD_WITHDRAW_BID: &str = "withdraw_bid";
/// Named constant for method `delegate`.
pub const METHOD_DELEGATE: &str = "delegate";
/// Named constant for method `undelegate`.
pub const METHOD_UNDELEGATE: &str = "undelegate";
/// Named constant for method `quash_bid`.
pub const METHOD_QUASH_BID: &str = "quash_bid";
/// Named constant for method `run_auction`.
pub const METHOD_RUN_AUCTION: &str = "run_auction";
/// Named constant for method `bond`.
pub const METHOD_BOND: &str = "bond";
/// Named constant for method `unbond`.
pub const METHOD_UNBOND: &str = "unbond";
/// Named constant for method `slash`.
pub const METHOD_SLASH: &str = "slash";
/// Named constant for method `release_founder_stake`.
pub const METHOD_RELEASE_FOUNDER_STAKE: &str = "release_founder_stake";
/// Named constant for method `distribute`.
pub const METHOD_DISTRIBUTE: &str = "distribute";

/// Storage for `Bids`.
pub const BIDS_KEY: &str = "bids";
/// Storage for `Delegators`.
pub const DELEGATORS_KEY: &str = "delegators";
/// Storage for `EraValidators`.
pub const ERA_VALIDATORS_KEY: &str = "era_validators";
/// Storage for `EraId`.
pub const ERA_ID_KEY: &str = "era_id";
/// Storage for `SeigniorageRecipientsSnapshot`.
pub const SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY: &str = "seigniorage_recipients_snapshot";
