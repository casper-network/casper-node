use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::shared::{system_config::SystemConfig, wasm_config::WasmConfig};

pub const DEFAULT_WASMLESS_TRANSFER_COST: u32 = 10_000;

/// Represents a protocol's data. Intended to be associated with a given protocol version.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ProtocolData {
    wasm_config: WasmConfig,
    system_config: SystemConfig,
}

/// Provides a default instance with non existing urefs and empty costs table.
///
/// Used in contexts where Handle Payment or Mint contract is not ready yet, and handle payment, and
/// mint installers are ran. For use with caution.
impl Default for ProtocolData {
    fn default() -> ProtocolData {
        ProtocolData {
            wasm_config: WasmConfig::default(),
            system_config: SystemConfig::default(),
        }
    }
}

impl ProtocolData {
    /// Creates a new `ProtocolData` value from a given `WasmCosts` value.
    pub fn new(wasm_config: WasmConfig, system_config: SystemConfig) -> Self {
        ProtocolData {
            wasm_config,
            system_config,
        }
    }

    /// Gets the `WasmConfig` value from a given [`ProtocolData`] value.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Gets the `SystemConfig` value from a given [`ProtocolData`] value.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }
}

impl ToBytes for ProtocolData {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.wasm_config.to_bytes()?);
        ret.append(&mut self.system_config.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.wasm_config.serialized_length() + self.system_config.serialized_length()
    }
}

impl FromBytes for ProtocolData {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (wasm_config, rem) = WasmConfig::from_bytes(bytes)?;
        let (system_config, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            ProtocolData {
                wasm_config,
                system_config,
            },
            rem,
        ))
    }
}

#[cfg(test)]
pub(crate) mod gens {
    use proptest::prop_compose;

    use crate::shared::{
        system_config::gens::system_config_arb, wasm_config::gens::wasm_config_arb,
    };

    use super::ProtocolData;

    prop_compose! {
        pub fn protocol_data_arb()(
            wasm_config in wasm_config_arb(),
            system_config in system_config_arb(),
        ) -> ProtocolData {
            ProtocolData {
                wasm_config,
                system_config,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_types::bytesrepr;

    use super::gens;

    proptest! {
        #[test]
        fn should_serialize_and_deserialize_with_arbitrary_values(
            protocol_data in gens::protocol_data_arb()
        ) {
            bytesrepr::test_serialization_roundtrip(&protocol_data);
        }
    }
}
