use alloc::collections::BTreeMap;

use super::PublicKey;
use crate::U512;

/// Delegators and associated bid "top-ups".
pub type DelegatedAmounts = BTreeMap<PublicKey, U512>;

/// Validators, mapped to a list of delegators and associated bid "top-ups".
pub type Delegators = BTreeMap<PublicKey, DelegatedAmounts>;
