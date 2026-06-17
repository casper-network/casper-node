use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

/// Supported EVM hardfork specifications for chainspec configuration.
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
    /// Base fee supplied to the EVM block context.
    pub base_fee: u64,
}

impl Default for EvmConfig {
    fn default() -> Self {
        EvmConfig {
            enabled: false,
            chain_id: 0,
            spec: EvmSpec::Prague,
            block_gas_limit: 30_000_000,
            base_fee: 0,
        }
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
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.enabled.write_bytes(writer)?;
        self.chain_id.write_bytes(writer)?;
        self.spec.write_bytes(writer)?;
        self.block_gas_limit.write_bytes(writer)?;
        self.base_fee.write_bytes(writer)
    }
}

impl FromBytes for EvmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (enabled, remainder) = bool::from_bytes(bytes)?;
        let (chain_id, remainder) = u64::from_bytes(remainder)?;
        let (spec, remainder) = EvmSpec::from_bytes(remainder)?;
        let (block_gas_limit, remainder) = u64::from_bytes(remainder)?;
        let (base_fee, remainder) = u64::from_bytes(remainder)?;
        Ok((
            EvmConfig {
                enabled,
                chain_id,
                spec,
                block_gas_limit,
                base_fee,
            },
            remainder,
        ))
    }
}
