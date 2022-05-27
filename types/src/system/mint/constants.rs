/// Named constant for `purse`.
pub const ARG_PURSE: &str = "purse";
/// Named constant for `amount`.
pub const ARG_AMOUNT: &str = "amount";
/// Named constant for `id`.
pub const ARG_ID: &str = "id";
/// Named constant for `to`.
pub const ARG_TO: &str = "to";
/// Named constant for `source`.
pub const ARG_SOURCE: &str = "source";
/// Named constant for `target`.
pub const ARG_TARGET: &str = "target";
/// Named constant for `round_seigniorage_rate` used in installer.
pub const ARG_ROUND_SEIGNIORAGE_RATE: &str = "round_seigniorage_rate";

/// Named constant for method `mint`.
pub const METHOD_MINT: &str = "mint";
/// Named constant for method `reduce_total_supply`.
pub const METHOD_REDUCE_TOTAL_SUPPLY: &str = "reduce_total_supply";
/// Named constant for (synthetic) method `create`
pub const METHOD_CREATE: &str = "create";
/// Named constant for method `balance`.
pub const METHOD_BALANCE: &str = "balance";
/// Named constant for method `transfer`.
pub const METHOD_TRANSFER: &str = "transfer";
/// Named constant for method `read_base_round_reward`.
pub const METHOD_READ_BASE_ROUND_REWARD: &str = "read_base_round_reward";
/// Named constant for method `mint_into_existing_purse`.
pub const METHOD_MINT_INTO_EXISTING_PURSE: &str = "mint_into_existing_purse";

/// Storage for mint contract hash.
pub const HASH_KEY: &str = "mint_hash";
/// Storage for mint access key.
pub const ACCESS_KEY: &str = "mint_access";
/// Storage for base round reward key.
pub const BASE_ROUND_REWARD_KEY: &str = "mint_base_round_reward";
/// Storage for mint total supply key.
pub const TOTAL_SUPPLY_KEY: &str = "total_supply";
/// Storage for mint round seigniorage rate.
pub const ROUND_SEIGNIORAGE_RATE_KEY: &str = "round_seigniorage_rate";
