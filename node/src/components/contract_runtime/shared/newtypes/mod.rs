//! Some newtypes.
mod macros;

use core::array::TryFromSliceError;
use std::{convert::TryFrom, fmt};

use blake2::{
    digest::{Input, VariableOutput},
    VarBlake2b,
};
use serde::Serialize;
use uuid::Uuid;

use types::bytesrepr::{self, FromBytes, ToBytes};

pub use types::BLAKE2B_DIGEST_LENGTH;

/// Represents a 32-byte BLAKE2b hash digest
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Blake2bHash([u8; BLAKE2B_DIGEST_LENGTH]);

impl Blake2bHash {
    /// Creates a 32-byte BLAKE2b hash digest from a given a piece of data
    pub fn new(data: &[u8]) -> Self {
        let mut ret = [0u8; BLAKE2B_DIGEST_LENGTH];
        // Safe to unwrap here because our digest length is constant and valid
        let mut hasher = VarBlake2b::new(BLAKE2B_DIGEST_LENGTH).unwrap();
        hasher.input(data);
        hasher.variable_result(|hash| ret.clone_from_slice(hash));
        Blake2bHash(ret)
    }

    /// Returns the underlying BLKAE2b hash bytes
    pub fn value(&self) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        self.0
    }

    /// Converts the underlying BLAKE2b hash digest array to a `Vec`
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl core::fmt::LowerHex for Blake2bHash {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let hex_string = base16::encode_lower(&self.value());
        if f.alternate() {
            write!(f, "0x{}", hex_string)
        } else {
            write!(f, "{}", hex_string)
        }
    }
}

impl core::fmt::UpperHex for Blake2bHash {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let hex_string = base16::encode_upper(&self.value());
        if f.alternate() {
            write!(f, "0x{}", hex_string)
        } else {
            write!(f, "{}", hex_string)
        }
    }
}

impl core::fmt::Display for Blake2bHash {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "Blake2bHash({:#x})", self)
    }
}

impl core::fmt::Debug for Blake2bHash {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", self)
    }
}

impl From<[u8; BLAKE2B_DIGEST_LENGTH]> for Blake2bHash {
    fn from(arr: [u8; BLAKE2B_DIGEST_LENGTH]) -> Self {
        Blake2bHash(arr)
    }
}

impl<'a> TryFrom<&'a [u8]> for Blake2bHash {
    type Error = TryFromSliceError;

    fn try_from(slice: &[u8]) -> Result<Blake2bHash, Self::Error> {
        <[u8; BLAKE2B_DIGEST_LENGTH]>::try_from(slice).map(Blake2bHash)
    }
}

impl Into<[u8; BLAKE2B_DIGEST_LENGTH]> for Blake2bHash {
    fn into(self) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        self.0
    }
}

impl ToBytes for Blake2bHash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Blake2bHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        FromBytes::from_bytes(bytes).map(|(arr, rem)| (Blake2bHash(arr), rem))
    }
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Serialize)]
pub struct CorrelationId(Uuid);

impl CorrelationId {
    pub fn new() -> CorrelationId {
        CorrelationId(Uuid::new_v4())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_nil()
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::components::contract_runtime::shared::{
        newtypes::{Blake2bHash, CorrelationId},
        utils,
    };
    use std::hash::{Hash, Hasher};

    #[test]
    fn should_be_able_to_generate_correlation_id() {
        let correlation_id = CorrelationId::new();

        assert_ne!(
            correlation_id.to_string(),
            "00000000-0000-0000-0000-000000000000",
            "should not be empty value"
        )
    }

    #[test]
    fn should_support_to_string() {
        let correlation_id = CorrelationId::new();

        assert!(
            !correlation_id.is_empty(),
            "correlation_id should be produce string"
        )
    }

    #[test]
    fn should_support_to_string_no_type_encasement() {
        let correlation_id = CorrelationId::new();

        let correlation_id_string = correlation_id.to_string();

        assert!(
            !correlation_id_string.starts_with("CorrelationId"),
            "correlation_id should just be the inner value without tuple name"
        )
    }

    #[test]
    fn should_support_to_json() {
        let correlation_id = CorrelationId::new();

        let correlation_id_json = utils::jsonify(correlation_id, false);

        assert!(
            !correlation_id_json.is_empty(),
            "correlation_id should be produce json"
        )
    }

    #[test]
    fn should_support_is_display() {
        let correlation_id = CorrelationId::new();

        let display = format!("{}", correlation_id);

        assert!(!display.is_empty(), "display should not be empty")
    }

    #[test]
    fn should_support_is_empty() {
        let correlation_id = CorrelationId::new();

        assert!(
            !correlation_id.is_empty(),
            "correlation_id should not be empty"
        )
    }

    #[test]
    fn should_create_unique_id_on_new() {
        let correlation_id_lhs = CorrelationId::new();
        let correlation_id_rhs = CorrelationId::new();

        assert_ne!(
            correlation_id_lhs, correlation_id_rhs,
            "correlation_ids should be distinct"
        );
    }

    #[test]
    fn should_support_clone() {
        let correlation_id = CorrelationId::new();

        let cloned = correlation_id;

        assert_eq!(correlation_id, cloned, "should be cloneable")
    }

    #[test]
    fn should_support_copy() {
        let correlation_id = CorrelationId::new();

        let cloned = correlation_id;

        assert_eq!(correlation_id, cloned, "should be cloneable")
    }

    #[test]
    fn should_support_hash() {
        let correlation_id = CorrelationId::new();

        let mut state = std::collections::hash_map::DefaultHasher::new();

        correlation_id.hash(&mut state);

        let hash = state.finish();

        assert!(hash > 0, "should be hashable");
    }

    #[test]
    fn should_display_blake2bhash_in_hex() {
        let hash = Blake2bHash([0u8; 32]);
        let hash_hex = format!("{}", hash);
        assert_eq!(
            hash_hex,
            "Blake2bHash(0x0000000000000000000000000000000000000000000000000000000000000000)"
        );
    }

    #[test]
    fn should_print_blake2bhash_lower_hex() {
        let hash = Blake2bHash([10u8; 32]);
        let hash_lower_hex = format!("{:x}", hash);
        assert_eq!(
            hash_lower_hex,
            "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"
        )
    }

    #[test]
    fn should_print_blake2bhash_upper_hex() {
        let hash = Blake2bHash([10u8; 32]);
        let hash_lower_hex = format!("{:X}", hash);
        assert_eq!(
            hash_lower_hex,
            "0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A"
        )
    }

    #[test]
    fn alternate_should_prepend_0x() {
        let hash = Blake2bHash([0u8; 32]);
        let hash_hex_alt = format!("{:#x}", hash);
        assert_eq!(
            hash_hex_alt,
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        )
    }
}
