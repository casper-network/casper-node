//! The registry of chainspec hash digests.

use std::{collections::BTreeMap, convert::TryFrom};

use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, Digest,
};

type BytesreprChainspecRegistry = BTreeMap<String, Digest>;

/// The chainspec registry.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub struct ChainspecRegistry {
    chainspec_raw_hash: Digest,
    genesis_accounts_raw_hash: Option<Digest>,
    global_state_raw_hash: Option<Digest>,
}

impl ChainspecRegistry {
    const CHAINSPEC_RAW_MAP_KEY: &'static str = "chainspec_raw";
    const GENESIS_ACCOUNTS_RAW_MAP_KEY: &'static str = "genesis_accounts_raw";
    const GLOBAL_STATE_RAW_MAP_KEY: &'static str = "global_state_raw";

    /// Returns a `ChainspecRegistry` constructed at genesis.
    pub fn new_with_genesis(
        chainspec_file_bytes: &[u8],
        genesis_accounts_file_bytes: &[u8],
    ) -> Self {
        ChainspecRegistry {
            chainspec_raw_hash: Digest::hash(chainspec_file_bytes),
            genesis_accounts_raw_hash: Some(Digest::hash(genesis_accounts_file_bytes)),
            global_state_raw_hash: None,
        }
    }

    /// Returns a `ChainspecRegistry` constructed at node upgrade.
    pub fn new_with_optional_global_state(
        chainspec_file_bytes: &[u8],
        global_state_file_bytes: Option<&[u8]>,
    ) -> Self {
        ChainspecRegistry {
            chainspec_raw_hash: Digest::hash(chainspec_file_bytes),
            genesis_accounts_raw_hash: None,
            global_state_raw_hash: global_state_file_bytes.map(Digest::hash),
        }
    }

    /// Returns the hash of the raw bytes of the chainspec.toml file.
    pub fn chainspec_raw_hash(&self) -> &Digest {
        &self.chainspec_raw_hash
    }

    /// Returns the hash of the raw bytes of the genesis accounts.toml file if it exists.
    pub fn genesis_accounts_raw_hash(&self) -> Option<&Digest> {
        self.genesis_accounts_raw_hash.as_ref()
    }

    /// Returns the hash of the raw bytes of the global_state.toml file if it exists.
    pub fn global_state_raw_hash(&self) -> Option<&Digest> {
        self.global_state_raw_hash.as_ref()
    }

    fn as_map(&self) -> BytesreprChainspecRegistry {
        let mut map = BTreeMap::new();
        map.insert(
            Self::CHAINSPEC_RAW_MAP_KEY.to_string(),
            self.chainspec_raw_hash,
        );
        if let Some(genesis_accounts_raw_hash) = self.genesis_accounts_raw_hash {
            map.insert(
                Self::GENESIS_ACCOUNTS_RAW_MAP_KEY.to_string(),
                genesis_accounts_raw_hash,
            );
        }
        if let Some(global_state_raw_hash) = self.global_state_raw_hash {
            map.insert(
                Self::GLOBAL_STATE_RAW_MAP_KEY.to_string(),
                global_state_raw_hash,
            );
        }
        map
    }
}

impl TryFrom<BytesreprChainspecRegistry> for ChainspecRegistry {
    type Error = bytesrepr::Error;

    fn try_from(map: BytesreprChainspecRegistry) -> Result<Self, Self::Error> {
        let chainspec_raw_hash = *map
            .get(Self::CHAINSPEC_RAW_MAP_KEY)
            .ok_or(bytesrepr::Error::Formatting)?;
        let genesis_accounts_raw_hash = map.get(Self::GENESIS_ACCOUNTS_RAW_MAP_KEY).copied();
        let global_state_raw_hash = map.get(Self::GLOBAL_STATE_RAW_MAP_KEY).copied();
        Ok(ChainspecRegistry {
            chainspec_raw_hash,
            genesis_accounts_raw_hash,
            global_state_raw_hash,
        })
    }
}

impl ToBytes for ChainspecRegistry {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.as_map().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.as_map().serialized_length()
    }
}

impl FromBytes for ChainspecRegistry {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (map, remainder) = BytesreprChainspecRegistry::from_bytes(bytes)?;
        let chainspec_registry = ChainspecRegistry::try_from(map)?;
        Ok((chainspec_registry, remainder))
    }
}

impl CLTyped for ChainspecRegistry {
    fn cl_type() -> CLType {
        BytesreprChainspecRegistry::cl_type()
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();

        let chainspec_file_bytes: [u8; 10] = rng.gen();

        let genesis_account_file_bytes: [u8; 10] = rng.gen();
        let chainspec_registry =
            ChainspecRegistry::new_with_genesis(&chainspec_file_bytes, &genesis_account_file_bytes);
        bytesrepr::test_serialization_roundtrip(&chainspec_registry);

        let global_state_file_bytes: [u8; 10] = rng.gen();
        let chainspec_registry = ChainspecRegistry::new_with_optional_global_state(
            &chainspec_file_bytes,
            Some(&global_state_file_bytes),
        );
        bytesrepr::test_serialization_roundtrip(&chainspec_registry);

        let chainspec_registry =
            ChainspecRegistry::new_with_optional_global_state(&chainspec_file_bytes, None);
        bytesrepr::test_serialization_roundtrip(&chainspec_registry);
    }
}
