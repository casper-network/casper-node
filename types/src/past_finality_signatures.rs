use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// TODO[RC]: This is a placeholder and should be replaced with the real `PastFinalitySignatures` structure
/// which is being implemented on the separate branch for the Zug consensus rewards. Also, the target file location may be different.
#[derive(
    Clone, DataSize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema, Debug,
)]
pub struct PastFinalitySignatures(pub(crate) Vec<u8>);

impl PastFinalitySignatures {
    /// Generates a random instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let count = rng.gen_range(0..11);
        Self(
            std::iter::repeat_with(|| rng.gen::<u8>())
                .take(count)
                .collect(),
        )
    }
}

impl Default for PastFinalitySignatures {
    fn default() -> Self {
        Self(vec![1, 2, 3, 4, 5])
    }
}

impl ToBytes for PastFinalitySignatures {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(Bytes::from(self.0.as_ref()).to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for PastFinalitySignatures {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, rest) = Bytes::from_bytes(bytes)?;
        Ok((PastFinalitySignatures(inner.into()), rest))
    }
}
