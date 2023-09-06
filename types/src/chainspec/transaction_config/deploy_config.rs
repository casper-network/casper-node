#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Motes,
};
#[cfg(any(feature = "testing", test))]
use crate::{testing::TestRng, U512};

/// The default  maximum number of motes that payment code execution can cost.
#[cfg(any(feature = "testing", test))]
pub const DEFAULT_MAX_PAYMENT_MOTES: u64 = 2_500_000_000;

/// Configuration values associated with deploys.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    /// Maximum amount any deploy can pay.
    pub max_payment_cost: Motes,
    /// Maximum time to live any deploy can specify.
    pub max_dependencies: u8,
    /// Maximum length in bytes of payment args per deploy.
    pub payment_args_max_length: u32,
    /// Maximum length in bytes of session args per deploy.
    pub session_args_max_length: u32,
}

#[cfg(any(feature = "testing", test))]
impl DeployConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let max_payment_cost = Motes::new(U512::from(rng.gen_range(1_000_000..1_000_000_000)));
        let max_dependencies = rng.gen();
        let payment_args_max_length = rng.gen();
        let session_args_max_length = rng.gen();

        DeployConfig {
            max_payment_cost,
            max_dependencies,
            payment_args_max_length,
            session_args_max_length,
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Default for DeployConfig {
    fn default() -> Self {
        DeployConfig {
            max_payment_cost: Motes::new(U512::from(DEFAULT_MAX_PAYMENT_MOTES)),
            max_dependencies: 10,
            payment_args_max_length: 1024,
            session_args_max_length: 1024,
        }
    }
}

impl ToBytes for DeployConfig {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.max_payment_cost.write_bytes(writer)?;
        self.max_dependencies.write_bytes(writer)?;
        self.payment_args_max_length.write_bytes(writer)?;
        self.session_args_max_length.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.max_payment_cost.value().serialized_length()
            + self.max_dependencies.serialized_length()
            + self.payment_args_max_length.serialized_length()
            + self.session_args_max_length.serialized_length()
    }
}

impl FromBytes for DeployConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_payment_cost, remainder) = Motes::from_bytes(bytes)?;
        let (max_dependencies, remainder) = u8::from_bytes(remainder)?;
        let (payment_args_max_length, remainder) = u32::from_bytes(remainder)?;
        let (session_args_max_length, remainder) = u32::from_bytes(remainder)?;
        let config = DeployConfig {
            max_payment_cost,
            max_dependencies,
            payment_args_max_length,
            session_args_max_length,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let config = DeployConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
