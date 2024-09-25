use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use {crate::testing::TestRng, rand::Rng};

/// Configuration values associated with deploys.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// Whether the chain is using the Casper v1 runtime.
    pub vm_casper_v1: bool,
    /// Whether the chain is using the Casper v2 runtime.
    pub vm_casper_v2: bool,
}

impl RuntimeConfig {
    #[cfg(any(feature = "testing", test))]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            vm_casper_v1: rng.gen(),
            vm_casper_v2: rng.gen(),
        }
    }
}

impl FromBytes for RuntimeConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), crate::bytesrepr::Error> {
        let (vm_casper_v1, rem) = bool::from_bytes(bytes)?;
        let (vm_casper_v2, rem) = bool::from_bytes(rem)?;
        Ok((
            RuntimeConfig {
                vm_casper_v1,
                vm_casper_v2,
            },
            rem,
        ))
    }
}

impl ToBytes for RuntimeConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.vm_casper_v1.serialized_length() + self.vm_casper_v2.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), crate::bytesrepr::Error> {
        self.vm_casper_v1.write_bytes(writer)?;
        self.vm_casper_v2.write_bytes(writer)
    }
}
