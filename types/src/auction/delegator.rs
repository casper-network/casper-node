use alloc::collections::BTreeMap;

use crate::{PublicKey, U512, URef};

/// Delegators and associated bid "top-ups".
pub type Delegations = BTreeMap<PublicKey, U512>;

/// Validators, mapped to a list of delegators and associated bid "top-ups".
pub type DelegationsMap = BTreeMap<PublicKey, Delegations>;

pub type Tally = BTreeMap<PublicKey, U512>;

pub type TallyMap = BTreeMap<PublicKey, Tally>;

pub type TotalDelegatorStakeMap = BTreeMap<PublicKey, U512>;

pub type RewardPerStakeMap = BTreeMap<PublicKey, U512>;

pub type DelegatorRewardPoolMap = BTreeMap<PublicKey, URef>;
