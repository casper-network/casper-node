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
    /// Frontier.
    Frontier,
    /// Frontier thawing.
    FrontierThawing,
    /// Homestead.
    Homestead,
    /// DAO fork.
    DaoFork,
    /// Tangerine Whistle.
    Tangerine,
    /// Spurious Dragon.
    SpuriousDragon,
    /// Byzantium.
    Byzantium,
    /// Constantinople.
    Constantinople,
    /// Petersburg.
    Petersburg,
    /// Istanbul.
    Istanbul,
    /// Muir Glacier.
    MuirGlacier,
    /// Berlin.
    Berlin,
    /// London.
    London,
    /// Arrow Glacier.
    ArrowGlacier,
    /// Gray Glacier.
    GrayGlacier,
    /// Paris, also known as the merge.
    Merge,
    /// Shanghai.
    Shanghai,
    /// Cancun.
    Cancun,
    /// Prague.
    #[default]
    Prague,
    /// Osaka.
    Osaka,
}

impl EvmSpec {
    fn tag(self) -> u8 {
        match self {
            EvmSpec::Frontier => 0,
            EvmSpec::FrontierThawing => 1,
            EvmSpec::Homestead => 2,
            EvmSpec::DaoFork => 3,
            EvmSpec::Tangerine => 4,
            EvmSpec::SpuriousDragon => 5,
            EvmSpec::Byzantium => 6,
            EvmSpec::Constantinople => 7,
            EvmSpec::Petersburg => 8,
            EvmSpec::Istanbul => 9,
            EvmSpec::MuirGlacier => 10,
            EvmSpec::Berlin => 11,
            EvmSpec::London => 12,
            EvmSpec::ArrowGlacier => 13,
            EvmSpec::GrayGlacier => 14,
            EvmSpec::Merge => 15,
            EvmSpec::Shanghai => 16,
            EvmSpec::Cancun => 17,
            EvmSpec::Prague => 18,
            EvmSpec::Osaka => 19,
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
            0 => EvmSpec::Frontier,
            1 => EvmSpec::FrontierThawing,
            2 => EvmSpec::Homestead,
            3 => EvmSpec::DaoFork,
            4 => EvmSpec::Tangerine,
            5 => EvmSpec::SpuriousDragon,
            6 => EvmSpec::Byzantium,
            7 => EvmSpec::Constantinople,
            8 => EvmSpec::Petersburg,
            9 => EvmSpec::Istanbul,
            10 => EvmSpec::MuirGlacier,
            11 => EvmSpec::Berlin,
            12 => EvmSpec::London,
            13 => EvmSpec::ArrowGlacier,
            14 => EvmSpec::GrayGlacier,
            15 => EvmSpec::Merge,
            16 => EvmSpec::Shanghai,
            17 => EvmSpec::Cancun,
            18 => EvmSpec::Prague,
            19 => EvmSpec::Osaka,
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
