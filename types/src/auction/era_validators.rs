use alloc::collections::BTreeMap;

use crate::{PublicKey, U512};

/// Weights of validators.
///
/// Weight in this context means a sum of their stakes.
pub type ValidatorWeights = BTreeMap<PublicKey, U512>;

/// Era index type.
pub type EraIndex = u64;

/// List of era validators
pub type EraValidators = BTreeMap<EraIndex, ValidatorWeights>;
