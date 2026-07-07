use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_rational::Ratio;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    U512,
};

/// The default number of wei represented by one mote.
pub const DEFAULT_WEI_PER_MOTE: u64 = 1_000_000_000;

/// The minimum number of wei represented by one mote.
pub const MINIMUM_WEI_PER_MOTE: u64 = DEFAULT_WEI_PER_MOTE;

/// Supported EVM hardfork specifications for chainspec configuration.
///
/// Variants are ordered by fork chronology; feature gates use ordinal
/// comparisons to apply functionality from a fork onward.
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EvmSpec {
    /// Prague.
    #[default]
    Prague,
}

impl EvmSpec {
    fn tag(self) -> u8 {
        match self {
            EvmSpec::Prague => 0,
        }
    }
}

impl ToBytes for EvmSpec {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(vec![self.tag()])
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        Ok(())
    }
}

impl FromBytes for EvmSpec {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let spec = match tag {
            0 => EvmSpec::Prague,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((spec, remainder))
    }
}

/// Chainspec configuration for EVM execution.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EvmConfig {
    /// Whether EVM execution is enabled.
    pub enabled: bool,
    /// EVM chain ID used for the `CHAINID` opcode and transaction validation.
    pub chain_id: u64,
    /// Hardfork specification used by the EVM executor.
    pub spec: EvmSpec,
    /// Per-block gas limit supplied to the EVM block context.
    pub block_gas_limit: u64,
    /// Base fee denominated in motes per EVM gas.
    pub base_fee: u64,
    /// Number of wei represented by one mote.
    pub wei_per_mote: u64,
}

impl Default for EvmConfig {
    fn default() -> Self {
        EvmConfig {
            enabled: false,
            chain_id: 0,
            spec: EvmSpec::Prague,
            block_gas_limit: 30_000_000,
            base_fee: 0,
            wei_per_mote: DEFAULT_WEI_PER_MOTE,
        }
    }
}

impl EvmConfig {
    /// Returns the EVM base fee denominated in wei.
    pub fn base_fee_wei(&self) -> u128 {
        u128::from(self.base_fee) * u128::from(self.wei_per_mote)
    }

    /// Converts an EVM gas cost, denominated in wei, to motes by rounding up.
    ///
    /// Rounding is applied after multiplying gas by price, so sub-mote totals
    /// are charged as one mote without overcharging each gas unit separately.
    pub fn gas_fee_motes(&self, gas: u64, gas_price_wei: u128) -> Option<U512> {
        if self.wei_per_mote == 0 {
            return None;
        }
        let fee_wei = U512::from(gas).checked_mul(U512::from(gas_price_wei))?;
        Some(
            Ratio::new(fee_wei, U512::from(self.wei_per_mote))
                .ceil()
                .to_integer(),
        )
    }
}

impl ToBytes for EvmConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.enabled.serialized_length()
            + self.chain_id.serialized_length()
            + self.spec.serialized_length()
            + self.block_gas_limit.serialized_length()
            + self.base_fee.serialized_length()
            + self.wei_per_mote.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.enabled.write_bytes(writer)?;
        self.chain_id.write_bytes(writer)?;
        self.spec.write_bytes(writer)?;
        self.block_gas_limit.write_bytes(writer)?;
        self.base_fee.write_bytes(writer)?;
        self.wei_per_mote.write_bytes(writer)
    }
}

impl FromBytes for EvmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (enabled, remainder) = bool::from_bytes(bytes)?;
        let (chain_id, remainder) = u64::from_bytes(remainder)?;
        let (spec, remainder) = EvmSpec::from_bytes(remainder)?;
        let (block_gas_limit, remainder) = u64::from_bytes(remainder)?;
        let (base_fee, remainder) = u64::from_bytes(remainder)?;
        let (wei_per_mote, remainder) = u64::from_bytes(remainder)?;
        Ok((
            EvmConfig {
                enabled,
                chain_id,
                spec,
                block_gas_limit,
                base_fee,
                wei_per_mote,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_scale_base_fee_to_wei() {
        let config = EvmConfig {
            base_fee: 3,
            ..Default::default()
        };

        assert_eq!(config.base_fee_wei(), 3_000_000_000u128);
    }

    #[test]
    fn should_convert_wei_gas_fee_to_motes() {
        let config = EvmConfig::default();

        assert_eq!(
            config.gas_fee_motes(21_000, u128::from(DEFAULT_WEI_PER_MOTE)),
            Some(U512::from(21_000))
        );
    }

    #[test]
    fn should_round_sub_mote_wei_gas_fee_up_to_one_mote() {
        let config = EvmConfig::default();

        assert_eq!(config.gas_fee_motes(1, 1), Some(U512::from(1)));
    }
}
