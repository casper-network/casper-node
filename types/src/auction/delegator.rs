use alloc::collections::BTreeMap;

use crate::{account::AccountHash, U512};

/// Delegators, mapped to a list of validators and associated bid "top-ups".
pub type Delegators = BTreeMap<AccountHash, BTreeMap<AccountHash, U512>>;
