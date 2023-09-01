use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Transform;
#[cfg(any(feature = "testing", test))]
use super::TransformKind;
use crate::bytesrepr::{self, FromBytes, ToBytes};

/// A log of all transforms produced during execution.
#[derive(Debug, Clone, Eq, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Effects(Vec<Transform>);

impl Effects {
    /// Constructs a new, empty `Effects`.
    pub const fn new() -> Self {
        Effects(vec![])
    }

    /// Returns a reference to the transforms.
    pub fn transforms(&self) -> &[Transform] {
        &self.0
    }

    /// Appends a transform.
    pub fn push(&mut self, transform: Transform) {
        self.0.push(transform)
    }

    /// Moves all elements from `other` into `self`.
    pub fn append(&mut self, mut other: Self) {
        self.0.append(&mut other.0);
    }

    /// Returns `true` if there are no transforms recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of transforms recorded.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Consumes `self`, returning the wrapped vec.
    pub fn value(self) -> Vec<Transform> {
        self.0
    }

    /// Returns a random `Effects`.
    #[cfg(any(feature = "testing", test))]
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        let mut effects = Effects::new();
        let transform_count = rng.gen_range(0..6);
        for _ in 0..transform_count {
            effects.push(Transform::new(rng.gen(), TransformKind::random(rng)));
        }
        effects
    }
}

impl ToBytes for Effects {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Effects {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transforms, remainder) = Vec::<Transform>::from_bytes(bytes)?;
        Ok((Effects(transforms), remainder))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::TestRng;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let effects = Effects::random(rng);
        bytesrepr::test_serialization_roundtrip(&effects);
    }
}
