use alloc::collections::BTreeMap;

use crate::{PublicKey, U512};

/// Weights of validators.
///
/// Weight in this context means a sum of their stakes.
pub type ValidatorWeights = BTreeMap<PublicKey, U512>;

/// Era index type.
pub type EraId = u64;

/// List of era validators
pub type EraValidators = BTreeMap<EraId, ValidatorWeights>;

#[cfg(test)]
mod tests {
    use crate::{auction::ValidatorWeights, public_key, U512};
    const ED25519_PUBLIC_KEY_LENGTH: usize = 32;

    #[test]
    fn json_roundtrip() {
        let mut item = ValidatorWeights::new();
        item.insert(
            public_key::PublicKey::Ed25519([0; ED25519_PUBLIC_KEY_LENGTH]),
            U512::from(10),
        );
        let json_string = serde_json::to_string_pretty(&item).unwrap();
        dbg!(json_string.clone());
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(item, decoded);
    }
}
