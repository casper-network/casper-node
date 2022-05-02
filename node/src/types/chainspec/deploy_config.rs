#[cfg(test)]
use std::str::FromStr;

use datasize::DataSize;
#[cfg(test)]
use num_traits::Zero;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_execution_engine::core::engine_state::MAX_PAYMENT_AMOUNT;
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    Motes, TimeDiff, U512,
};

#[derive(Copy, Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    pub(crate) max_payment_cost: Motes,
    pub(crate) max_ttl: TimeDiff,
    pub(crate) max_dependencies: u8,
    pub(crate) max_block_size: u32,
    pub(crate) max_deploy_size: u32,
    pub(crate) block_max_deploy_count: u32,
    pub(crate) block_max_transfer_count: u32,
    pub(crate) block_max_approval_count: u32,
    pub(crate) block_gas_limit: u64,
    pub(crate) payment_args_max_length: u32,
    pub(crate) session_args_max_length: u32,
    pub(crate) native_transfer_minimum_motes: u64,
}

#[cfg(test)]
impl DeployConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let max_payment_cost = Motes::new(U512::from(rng.gen_range(1_000_000..1_000_000_000)));
        let max_ttl = TimeDiff::from(rng.gen_range(60_000..3_600_000));
        let max_dependencies = rng.gen();
        let max_block_size = rng.gen_range(1_000_000..1_000_000_000);
        let max_deploy_size = rng.gen_range(100_000..1_000_000);
        let block_max_deploy_count = rng.gen();
        let block_max_transfer_count = rng.gen();
        let block_max_approval_count = rng.gen();
        let block_gas_limit = rng.gen_range(100_000_000_000..1_000_000_000_000_000);
        let payment_args_max_length = rng.gen();
        let session_args_max_length = rng.gen();
        let native_transfer_minimum_motes =
            rng.gen_range(MAX_PAYMENT_AMOUNT..1_000_000_000_000_000);

        DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            max_deploy_size,
            block_max_deploy_count,
            block_max_transfer_count,
            block_max_approval_count,
            block_gas_limit,
            payment_args_max_length,
            session_args_max_length,
            native_transfer_minimum_motes,
        }
    }
}

#[cfg(test)]
impl Default for DeployConfig {
    fn default() -> Self {
        DeployConfig {
            max_payment_cost: Motes::zero(),
            max_ttl: TimeDiff::from_str("1day").unwrap(),
            max_dependencies: 10,
            max_block_size: 10_485_760,
            max_deploy_size: 1_048_576,
            block_max_deploy_count: 10,
            block_max_transfer_count: 1000,
            block_max_approval_count: 2600,
            block_gas_limit: 10_000_000_000_000,
            payment_args_max_length: 1024,
            session_args_max_length: 1024,
            native_transfer_minimum_motes: MAX_PAYMENT_AMOUNT,
        }
    }
}

impl ToBytes for DeployConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.max_payment_cost.value().to_bytes()?);
        buffer.extend(self.max_ttl.to_bytes()?);
        buffer.extend(self.max_dependencies.to_bytes()?);
        buffer.extend(self.max_block_size.to_bytes()?);
        buffer.extend(self.max_deploy_size.to_bytes()?);
        buffer.extend(self.block_max_deploy_count.to_bytes()?);
        buffer.extend(self.block_max_transfer_count.to_bytes()?);
        buffer.extend(self.block_max_approval_count.to_bytes()?);
        buffer.extend(self.block_gas_limit.to_bytes()?);
        buffer.extend(self.payment_args_max_length.to_bytes()?);
        buffer.extend(self.session_args_max_length.to_bytes()?);
        buffer.extend(self.native_transfer_minimum_motes.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.max_payment_cost.value().serialized_length()
            + self.max_ttl.serialized_length()
            + self.max_dependencies.serialized_length()
            + self.max_block_size.serialized_length()
            + self.max_deploy_size.serialized_length()
            + self.block_max_deploy_count.serialized_length()
            + self.block_max_transfer_count.serialized_length()
            + self.block_max_approval_count.serialized_length()
            + self.block_gas_limit.serialized_length()
            + self.payment_args_max_length.serialized_length()
            + self.session_args_max_length.serialized_length()
            + self.native_transfer_minimum_motes.serialized_length()
    }
}

impl FromBytes for DeployConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_payment_cost, remainder) = U512::from_bytes(bytes)?;
        let max_payment_cost = Motes::new(max_payment_cost);
        let (max_ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (max_dependencies, remainder) = u8::from_bytes(remainder)?;
        let (max_block_size, remainder) = u32::from_bytes(remainder)?;
        let (max_deploy_size, remainder) = u32::from_bytes(remainder)?;
        let (block_max_deploy_count, remainder) = u32::from_bytes(remainder)?;
        let (block_max_transfer_count, remainder) = u32::from_bytes(remainder)?;
        let (block_max_approval_count, remainder) = u32::from_bytes(remainder)?;
        let (block_gas_limit, remainder) = u64::from_bytes(remainder)?;
        let (payment_args_max_length, remainder) = u32::from_bytes(remainder)?;
        let (session_args_max_length, remainder) = u32::from_bytes(remainder)?;
        let (native_transfer_minimum_motes, remainder) = u64::from_bytes(remainder)?;
        let config = DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            max_deploy_size,
            block_max_deploy_count,
            block_max_transfer_count,
            block_max_approval_count,
            block_gas_limit,
            payment_args_max_length,
            session_args_max_length,
            native_transfer_minimum_motes,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let config = DeployConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }

    #[test]
    fn toml_roundtrip() {
        let mut rng = crate::new_rng();
        let config = DeployConfig::random(&mut rng);
        let encoded = toml::to_string_pretty(&config).unwrap();
        let decoded = toml::from_str(&encoded).unwrap();
        assert_eq!(config, decoded);
    }
}
