/// Named constant for `purse`.
pub const ARG_PURSE: &str = "purse";
/// Named constant for `amount`.
pub const ARG_AMOUNT: &str = "amount";
/// Named constant for `source`.
pub const ARG_ACCOUNT: &str = "account";
/// Named constant for `target`.
pub const ARG_TARGET: &str = "target";

/// Named constant for method `get_payment_purse`.
pub const METHOD_GET_PAYMENT_PURSE: &str = "get_payment_purse";
/// Named constant for method `set_refund_purse`.
pub const METHOD_SET_REFUND_PURSE: &str = "set_refund_purse";
/// Named constant for method `get_refund_purse`.
pub const METHOD_GET_REFUND_PURSE: &str = "get_refund_purse";
/// Named constant for method `finalize_payment`.
pub const METHOD_FINALIZE_PAYMENT: &str = "finalize_payment";

/// Storage for proof of stake payment purse.
pub const POS_PAYMENT_PURSE: &str = "pos_payment_purse";
/// Storage for proof of stake contract hash.
pub const HASH_KEY: &str = "pos_hash";
/// Storage for proof of stake access key.
pub const ACCESS_KEY: &str = "pos_access";

/// The uref name where the PoS accepts payment for computation on behalf of validators.
pub const PAYMENT_PURSE_KEY: &str = "pos_payment_purse";

/// The uref name where the PoS will refund unused payment back to the user. The uref this name
/// corresponds to is set by the user.
pub const REFUND_PURSE_KEY: &str = "pos_refund_purse";
