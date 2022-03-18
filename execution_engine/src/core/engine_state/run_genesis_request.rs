//! Support for legacy `RunGenesisRequest`/`GenesisConfig` that is not used by the node's contract
//! runtime component anymore.
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use casper_hashing::Digest;
use casper_types::ProtocolVersion;

use super::{genesis::ExecConfig, ChainspecRegistry};

/// Represents a genesis request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunGenesisRequest {
    genesis_config_hash: Digest,
    protocol_version: ProtocolVersion,
    ee_config: ExecConfig,
    chainspec_registry: ChainspecRegistry,
}

impl RunGenesisRequest {
    /// Creates new genesis request.
    pub fn new(
        genesis_config_hash: Digest,
        protocol_version: ProtocolVersion,
        ee_config: ExecConfig,
        chainspec_registry: ChainspecRegistry,
    ) -> RunGenesisRequest {
        RunGenesisRequest {
            genesis_config_hash,
            protocol_version,
            ee_config,
            chainspec_registry,
        }
    }

    /// Returns genesis config hash.
    pub fn genesis_config_hash(&self) -> Digest {
        self.genesis_config_hash
    }

    /// Returns protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns EE config.
    pub fn ee_config(&self) -> &ExecConfig {
        &self.ee_config
    }

    /// Returns a reference to the chainspec registry.
    pub fn chainspec_registry(&self) -> &ChainspecRegistry {
        &self.chainspec_registry
    }

    /// Returns a EE config and consumes the object.
    pub fn take_ee_config(self) -> ExecConfig {
        self.ee_config
    }
}

impl Distribution<RunGenesisRequest> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> RunGenesisRequest {
        let input: [u8; 32] = rng.gen();
        let genesis_config_hash = Digest::hash(&input);
        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());
        let ee_config = rng.gen();

        let chainspec_file_bytes: [u8; 10] = rng.gen();
        let genesis_account_file_bytes: [u8; 15] = rng.gen();
        let chainspec_registry =
            ChainspecRegistry::new_with_genesis(&chainspec_file_bytes, &genesis_account_file_bytes);
        RunGenesisRequest::new(
            genesis_config_hash,
            protocol_version,
            ee_config,
            chainspec_registry,
        )
    }
}
