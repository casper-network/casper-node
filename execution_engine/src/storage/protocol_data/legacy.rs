///! Routine for decoding a legacy [`ProtocolData`] serialization format.
mod host_function_costs;
mod opcode_costs;
mod storage_costs;
mod system_config;
mod wasm_config;

use casper_types::{
    bytesrepr::{self, FromBytes},
    HashAddr, ProtocolVersion,
};

use self::{system_config::LegacySystemConfig, wasm_config::LegacyWasmConfig};

/// Protocol versions lower than or equal to this value should be deserialized with a legacy format.
pub const LEGACY_PROTOCOL_DATA_VERSION: ProtocolVersion = ProtocolVersion::from_parts(1, 2, 0);

use super::ProtocolData;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct LegacyProtocolData(ProtocolData);

impl From<LegacyProtocolData> for ProtocolData {
    fn from(legacy_protocol_data: LegacyProtocolData) -> Self {
        legacy_protocol_data.0
    }
}

impl FromBytes for LegacyProtocolData {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (legacy_wasm_config, rem) = LegacyWasmConfig::from_bytes(bytes)?;
        let (legacy_system_config, rem) = LegacySystemConfig::from_bytes(rem)?;
        let (mint, rem) = HashAddr::from_bytes(rem)?;
        let (handle_payment, rem) = HashAddr::from_bytes(rem)?;
        let (standard_payment, rem) = HashAddr::from_bytes(rem)?;
        let (auction, rem) = HashAddr::from_bytes(rem)?;

        let protocol_data = ProtocolData {
            wasm_config: legacy_wasm_config.into(),
            mint: mint.into(),
            handle_payment: handle_payment.into(),
            standard_payment: standard_payment.into(),
            auction: auction.into(),
            system_config: legacy_system_config.into(),
        };

        Ok((LegacyProtocolData(protocol_data), rem))
    }
}
