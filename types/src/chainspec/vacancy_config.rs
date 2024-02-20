#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::Rng;
use serde::{Deserialize, Serialize};
use crate::bytesrepr;
use crate::bytesrepr::{Error, FromBytes, ToBytes};
use crate::testing::TestRng;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct VacancyConfig {
    pub upper_threshold: u64,
    pub lower_threshold: u64,
    pub max_gas_price: u8,
    pub min_gas_price: u8,
}

impl VacancyConfig {
    pub(crate) fn new(
        upper_threshold: u64,
        lower_threshold: u64,
        max_gas_price: u8,
        min_gas_price: u8,
    ) -> Self {
        Self {
            upper_threshold,
            lower_threshold,
            max_gas_price,
            min_gas_price,
        }
    }


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

impl ToBytes for VacancyConfig{
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
        Ok((Self {
            upper_threshold,
            lower_threshold,
            max_gas_price,
            min_gas_price,
        }, remainder))
    }
}

