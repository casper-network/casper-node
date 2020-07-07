use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use std::{convert::TryInto, iter};

use super::genesis::ExecConfig;
use crate::components::contract_runtime::shared::newtypes::{Blake2bHash, BLAKE2B_DIGEST_LENGTH};
use types::ProtocolVersion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunGenesisRequest {
    genesis_config_hash: Blake2bHash,
    protocol_version: ProtocolVersion,
    ee_config: ExecConfig,
}

impl RunGenesisRequest {
    pub fn new(
        genesis_config_hash: Blake2bHash,
        protocol_version: ProtocolVersion,
        ee_config: ExecConfig,
    ) -> RunGenesisRequest {
        RunGenesisRequest {
            genesis_config_hash,
            protocol_version,
            ee_config,
        }
    }

    pub fn genesis_config_hash(&self) -> Blake2bHash {
        self.genesis_config_hash
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn ee_config(&self) -> &ExecConfig {
        &self.ee_config
    }

    pub fn take_ee_config(self) -> ExecConfig {
        self.ee_config
    }
}

impl Distribution<RunGenesisRequest> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> RunGenesisRequest {
        let hash_bytes: [u8; BLAKE2B_DIGEST_LENGTH] = {
            let bytes: Vec<u8> = iter::repeat(())
                .map(|_| rng.gen())
                .take(BLAKE2B_DIGEST_LENGTH)
                .collect();
            bytes.as_slice().try_into().expect("should convert")
        };

        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());
        let ee_config = rng.gen();

        RunGenesisRequest::new(hash_bytes.into(), protocol_version, ee_config)
    }
}
