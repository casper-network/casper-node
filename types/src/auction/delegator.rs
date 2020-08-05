use alloc::collections::BTreeMap;

use crate::{PublicKey, U512};

/// Delegators and associated bid "top-ups".
pub type Delegations = BTreeMap<AccountHash, U512>;

/// Validators, mapped to a list of delegators and associated bid "top-ups".
pub type DelegationsMap = BTreeMap<AccountHash, Delegations>;

pub type Tally = BTreeMap<AccountHash, U512>;

pub type TallyMap = BTreeMap<AccountHash, Tally>;

pub type TotalDelegatorStakeMap = BTreeMap<AccountHash, U512>;

pub type RewardPerStakeMap = BTreeMap<AccountHash, U512>;

pub type DelegatorRewardPoolMap = BTreeMap<AccountHash, URef>;
