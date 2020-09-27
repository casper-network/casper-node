use alloc::collections::BTreeMap;

use crate::{PublicKey, U512};

/// Representation of delegation rate of tokens. Fraction of 1 in trillionths (12 decimal places).
pub type DelegationRate = u64;

/// Delegators and associated bid "top-ups".
pub type DelegatedAmounts = BTreeMap<PublicKey, U512>;

/// Validators, mapped to a list of delegators and associated bid "top-ups".
pub type Delegators = BTreeMap<PublicKey, DelegatedAmounts>;

/// Validators mapped to Delegators mapped to their reward amounts.
pub type DelegatorRewardMap = BTreeMap<PublicKey, BTreeMap<PublicKey, U512>>;

/// Validators mapped to their reward amounts.
pub type ValidatorRewardMap = BTreeMap<PublicKey, U512>;
