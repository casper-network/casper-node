use alloc::collections::BTreeMap;

use crate::{PublicKey, U512};

/// Rewards distributed to validators.
pub enum Rewards<'a> {
    /// Rewards for version 1, associate a ratio to each validator.
    V1(&'a BTreeMap<PublicKey, u64>),
    /// Rewards for version 1, associate a tokens amount to each validator.
    V2(&'a BTreeMap<PublicKey, U512>),
}
