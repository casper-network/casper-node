#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr,
    bytesrepr::{Error, FromBytes, ToBytes},
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

/// The configuration to determine gas price based on block vacancy.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct VacancyConfig {
    /// The upper threshold to determine an increment in gas price
    pub upper_threshold: u64,
    /// The lower threshold to determine a decrement in gas price
    pub lower_threshold: u64,
    /// The upper limit of the gas price.
    pub max_gas_price: u8,
    /// The lower limit of the gas price.
    pub min_gas_price: u8,
}

impl Default for VacancyConfig {
    fn default() -> Self {
        Self {
            upper_threshold: 90,
            lower_threshold: 50,
            max_gas_price: 3,
            min_gas_price: 1,
        }
    }
}

impl VacancyConfig {
    /// Returns a random [`VacancyConfig`]
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            upper_threshold: rng.gen_range(49..100),
            lower_threshold: rng.gen_range(0..50),
            max_gas_price: rng.gen_range(3..5),
            min_gas_price: rng.gen_range(1..3),
        }
    }
}

impl ToBytes for VacancyConfig {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.upper_threshold.write_bytes(writer)?;
        self.lower_threshold.write_bytes(writer)?;
        self.max_gas_price.write_bytes(writer)?;
        self.min_gas_price.write_bytes(writer)
    }
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }
    fn serialized_length(&self) -> usize {
        self.upper_threshold.serialized_length()
            + self.lower_threshold.serialized_length()
            + self.max_gas_price.serialized_length()
            + self.min_gas_price.serialized_length()
    }
}

impl FromBytes for VacancyConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (upper_threshold, remainder) = u64::from_bytes(bytes)?;
        let (lower_threshold, remainder) = u64::from_bytes(remainder)?;
        let (max_gas_price, remainder) = u8::from_bytes(remainder)?;
        let (min_gas_price, remainder) = u8::from_bytes(remainder)?;
        Ok((
            Self {
                upper_threshold,
                lower_threshold,
                max_gas_price,
                min_gas_price,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let config = VacancyConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
