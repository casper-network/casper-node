use alloc::{collections::BTreeMap, vec::Vec};

use crate::{PublicKey, U512};

/// Rewards distributed to validators.
#[derive(Debug)]
pub enum Rewards<'a> {
    /// Rewards for version 1, associate a ratio to each validator.
    V1(&'a BTreeMap<PublicKey, u64>),
    /// Rewards for version 1, associate a tokens amount to each validator.
    V2(&'a BTreeMap<PublicKey, Vec<U512>>),
}
