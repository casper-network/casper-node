use alloc::collections::BTreeMap;

use crate::{account::AccountHash, U512};

/// Delegators and associated bid "top-ups".
pub type DelegatedAmounts = BTreeMap<AccountHash, U512>;

/// Validators, mapped to a list of delegators and associated bid "top-ups".
pub type Delegators = BTreeMap<AccountHash, DelegatedAmounts>;
