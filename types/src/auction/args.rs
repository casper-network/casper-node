use super::EraId;

/// Maximum number of era validator slots.
pub const AUCTION_SLOTS: usize = 5;

/// Number of eras before an auction actually defines the set of validators.
pub const AUCTION_DELAY: u64 = 3;

/// Number of eras to keep track of in past.
pub const SNAPSHOT_SIZE: usize = AUCTION_DELAY as usize + 1;

/// Initial value of era id we start at genesis.
pub const INITIAL_ERA_ID: EraId = 0;

/// Named constant for `amount`.
pub const ARG_AMOUNT: &str = "amount";
/// Named constant for `delegation_rate`.
pub const ARG_DELEGATION_RATE: &str = "delegation_rate";
/// Named constant for `account_hash`.
pub const ARG_PUBLIC_KEY: &str = "public_key";
/// Named constant for `purse`.
pub const ARG_PURSE: &str = "purse";
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

/// Named constant for method `release_founder`.
pub const METHOD_RELEASE_FOUNDER: &str = "release_founder";
/// Named constant for method `read_winners`.
pub const METHOD_READ_WINNERS: &str = "read_winners";
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
/// Named constant for method `read_era_id`.
pub const METHOD_READ_ERA_ID: &str = "read_era_id";

/// Storage for `FoundingValidators`.
pub const FOUNDING_VALIDATORS_KEY: &str = "founding_validators";
/// Storage for `ActiveBids`.
pub const ACTIVE_BIDS_KEY: &str = "active_bids";
/// Storage for `Delegators`.
pub const DELEGATORS_KEY: &str = "delegators";
/// Storage for `EraValidators`.
pub const ERA_VALIDATORS_KEY: &str = "era_validators";
/// Storage for `EraId`.
pub const ERA_ID_KEY: &str = "era_id";
/// Storage for `SeigniorageRecipientsSnapshot`.
pub const SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY: &str = "seigniorage_recipients_snapshot";
