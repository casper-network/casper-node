//! Named EVM contracts for configuration discovery, without executor dependencies.
//!
//! Executor and storage tests verify these maps against the selected precompile
//! provider and the system-contract installation registry.

#[cfg(any(feature = "std", test))]
use alloc::{collections::BTreeMap, string::String, string::ToString};

use super::Address;
#[cfg(any(feature = "std", test))]
use crate::{EvmConfig, EvmSpec};

/// EIP-2935 block hash history contract address.
pub const HISTORY_STORAGE_ADDRESS: Address = Address::new([
    0x00, 0x00, 0xf9, 0x08, 0x27, 0xf1, 0xc5, 0x3a, 0x10, 0xcb, 0x7a, 0x02, 0x33, 0x5b, 0x17, 0x53,
    0x20, 0x00, 0x29, 0x35,
]);

/// EIP-4788 beacon roots contract address.
pub const BEACON_ROOTS_ADDRESS: Address = Address::new([
    0x00, 0x0f, 0x3d, 0xf6, 0xd7, 0x32, 0x80, 0x7e, 0xf1, 0x31, 0x9f, 0xb7, 0xb8, 0xbb, 0x85, 0x22,
    0xd0, 0xbe, 0xac, 0x02,
]);

#[cfg(any(feature = "std", test))]
impl EvmConfig {
    /// Names and addresses of the configured fork's active precompiles.
    pub fn active_precompiles(&self) -> BTreeMap<String, Address> {
        if !self.enabled {
            return BTreeMap::new();
        }
        let precompiles = match self.spec {
            EvmSpec::Osaka => [
                ("ECREC", 1u16),
                ("SHA256", 2),
                ("RIPEMD160", 3),
                ("ID", 4),
                ("MODEXP", 5),
                ("BN254_ADD", 6),
                ("BN254_MUL", 7),
                ("BN254_PAIRING", 8),
                ("BLAKE2F", 9),
                ("KZG_POINT_EVALUATION", 10),
                ("BLS12_G1ADD", 11),
                ("BLS12_G1MSM", 12),
                ("BLS12_G2ADD", 13),
                ("BLS12_G2MSM", 14),
                ("BLS12_PAIRING_CHECK", 15),
                ("BLS12_MAP_FP_TO_G1", 16),
                ("BLS12_MAP_FP2_TO_G2", 17),
                ("P256VERIFY", 256),
            ],
        };
        precompiles
            .into_iter()
            .map(|(name, number)| {
                let mut address = [0; 20];
                address[18..].copy_from_slice(&number.to_be_bytes());
                (name.to_string(), Address::new(address))
            })
            .collect()
    }

    /// Names and addresses of system contracts installed for this configuration.
    /// Canonical utility preinstalls are separate from system contracts.
    pub fn active_system_contracts(&self) -> BTreeMap<String, Address> {
        if !self.enabled {
            return BTreeMap::new();
        }
        let contracts = match self.spec {
            EvmSpec::Osaka => [
                ("BEACON_ROOTS_ADDRESS", BEACON_ROOTS_ADDRESS),
                ("HISTORY_STORAGE_ADDRESS", HISTORY_STORAGE_ADDRESS),
            ],
        };
        contracts
            .into_iter()
            .map(|(name, address)| (name.to_string(), address))
            .collect()
    }
}
