use super::EraId;
use crate::account::AccountHash;

/// System account hash.
pub const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0; 32]);

/// Number of eras before an auction actually defines the set of validators.
//pub const AUCTION_DELAY: u64 = 3;

/// Number of eras to keep track of in past.
pub const SNAPSHOT_SIZE: usize = 4;

/// Initial value of era id we start at genesis.
pub const INITIAL_ERA_ID: EraId = 0;

/// Default lock period for new bid entries represented in eras.
pub const DEFAULT_LOCKED_FUNDS_PERIOD: EraId = 15;

/// Delegation rate is a fraction between 0-1. Validator sets the delegation rate
/// in integer terms, which is then divided by the denominator to obtain the fraction.
pub const DELEGATION_RATE_DENOMINATOR: u64 = 1_000_000_000_000;

/// We use one trillion as a block reward unit because it's large enough to allow precise
/// fractions, and small enough for many block rewards to fit into a u64.
pub const BLOCK_REWARD: u64 = 1_000_000_000_000;

/// Total validator slots allowed.
pub const VALIDATOR_SLOTS_KEY: &str = "validator_slots";

/// Amount of auction delay.
pub const AUCTION_DELAY_KEY: &str = "auction_delay";

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
/// Named constant for `validator_public_key`.
pub const ARG_VALIDATOR_PUBLIC_KEY: &str = "validator_public_key";
/// Named constant for `delegator_public_key`.
pub const ARG_DELEGATOR_PUBLIC_KEY: &str = "delegator_public_key";
/// Named constant for `target_purse`.
pub const ARG_TARGET_PURSE: &str = "target_purse";
/// Named constant for `unbond_purse`.
pub const ARG_UNBOND_PURSE: &str = "unbond_purse";
/// Named constant for `validator_slots` argument.
pub const ARG_VALIDATOR_SLOTS: &str = VALIDATOR_SLOTS_KEY;
/// Named constant for `mint_contract_package_hash`
pub const ARG_MINT_CONTRACT_PACKAGE_HASH: &str = "mint_contract_package_hash";
/// Named constant for `genesis_validators`
pub const ARG_GENESIS_VALIDATORS: &str = "genesis_validators";
/// Named constant of `auction_delay`
pub const ARG_AUCTION_DELAY: &str = "auction_delay";
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
/// Named constant for method `run_auction`.
pub const METHOD_RUN_AUCTION: &str = "run_auction";
/// Named constant for method `slash`.
pub const METHOD_SLASH: &str = "slash";
/// Named constant for method `release_founder_stake`.
pub const METHOD_RELEASE_FOUNDER_STAKE: &str = "release_founder_stake";
/// Named constant for method `distribute`.
pub const METHOD_DISTRIBUTE: &str = "distribute";
/// Named constant for method `withdraw_delegator_reward`.
pub const METHOD_WITHDRAW_DELEGATOR_REWARD: &str = "withdraw_delegator_reward";
/// Named constant for method `withdraw_validator_reward`.
pub const METHOD_WITHDRAW_VALIDATOR_REWARD: &str = "withdraw_validator_reward";
/// Named constant for method `read_era_id`.
pub const METHOD_READ_ERA_ID: &str = "read_era_id";

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
/// Storage for delegator reward purse
pub const DELEGATOR_REWARD_PURSE: &str = "delegator_reward_purse";
/// Storage for validator reward purse
pub const VALIDATOR_REWARD_PURSE: &str = "validator_reward_purse";
/// Storage for `DelegatorRewardMap`.
pub const DELEGATOR_REWARD_MAP: &str = "delegator_reward_map";
/// Storage for `ValidatorRewardMap`.
pub const VALIDATOR_REWARD_MAP: &str = "validator_reward_map";
