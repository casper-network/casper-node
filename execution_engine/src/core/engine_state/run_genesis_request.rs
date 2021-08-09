use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use casper_types::{CheckSummedHex, CheckSummedHexForm, ProtocolVersion};

use super::genesis::ExecConfig;
use crate::shared::newtypes::Blake2bHash;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunGenesisRequest {
    #[serde(with = "CheckSummedHexForm::<Blake2bHash>")]
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
        let input: [u8; 32] = rng.gen();
        let genesis_config_hash = Blake2bHash::new(&input);
        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());
        let ee_config = rng.gen();
        RunGenesisRequest::new(genesis_config_hash, protocol_version, ee_config)
    }
}
