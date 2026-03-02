use num_rational::Ratio;
#[cfg(test)]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

use casper_types::{
    execution::Effects, ChainspecRegistry, Digest, GenesisAccount, GenesisConfig, GenesisValidator,
    ProtocolVersion, PublicKey,
};

use crate::system::genesis::GenesisError;

/// Represents a configuration of a genesis process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisRequest {
    chainspec_hash: Digest,
    protocol_version: ProtocolVersion,
    config: GenesisConfig,
    chainspec_registry: ChainspecRegistry,
}

impl GenesisRequest {
    /// Creates a new genesis config object.
    pub fn new(
        chainspec_hash: Digest,
        protocol_version: ProtocolVersion,
        config: GenesisConfig,
        chainspec_registry: ChainspecRegistry,
    ) -> Self {
        GenesisRequest {
            chainspec_hash,
            protocol_version,
            config,
            chainspec_registry,
        }
    }

    /// Set enable entity.
    pub fn set_enable_entity(&mut self, enable: bool) {
        self.config.set_enable_entity(enable);
    }

    /// Push genesis validator.
    pub fn push_genesis_account(&mut self, genesis_account: GenesisAccount) {
        self.config.push_account(genesis_account);
    }

    /// Push genesis validator.
    pub fn push_genesis_validator(
        &mut self,
        public_key: &PublicKey,
        genesis_validator: GenesisValidator,
    ) {
        self.config
            .push_genesis_validator(public_key, genesis_validator);
    }

    /// Returns chainspec_hash.
    pub fn chainspec_hash(&self) -> Digest {
        self.chainspec_hash
    }

    /// Returns protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns configuration details of the genesis process.
    pub fn config(&self) -> &GenesisConfig {
        &self.config
    }

    /// Returns chainspec registry.
    pub fn chainspec_registry(&self) -> &ChainspecRegistry {
        &self.chainspec_registry
    }

    /// Push a rewards ratio into the genesis request.
    pub fn push_rewards_ratio(&mut self, rewards_ratio: Ratio<u64>) {
        self.config.push_rewards_ratio(rewards_ratio)
    }
}

#[cfg(test)]
impl Distribution<GenesisRequest> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisRequest {
        let input: [u8; 32] = rng.gen();
        let chainspec_hash = Digest::hash(input);
        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());
        let config = rng.gen();

        let chainspec_file_bytes: [u8; 10] = rng.gen();
        let genesis_account_file_bytes: [u8; 15] = rng.gen();
        let chainspec_registry =
            ChainspecRegistry::new_with_genesis(&chainspec_file_bytes, &genesis_account_file_bytes);
        GenesisRequest::new(chainspec_hash, protocol_version, config, chainspec_registry)
    }
}

/// Represents a result of a `genesis` request.
#[derive(Debug, Clone)]
pub enum GenesisResult {
    /// Genesis fatal.
    Fatal(String),
    /// Genesis failure.
    Failure(GenesisError),
    /// Genesis success.
    Success {
        /// State hash after genesis is committed to the global state.
        post_state_hash: Digest,
        /// Effects of genesis.
        effects: Effects,
    },
}

impl GenesisResult {
    /// Is success.
    pub fn is_success(&self) -> bool {
        matches!(self, GenesisResult::Success { .. })
    }

    /// Returns a Result matching the original api for this functionality.
    pub fn as_legacy(self) -> Result<(Digest, Effects), Box<GenesisError>> {
        match self {
            GenesisResult::Fatal(_) => Err(Box::new(GenesisError::StateUninitialized)),
            GenesisResult::Failure(err) => Err(Box::new(err)),
            GenesisResult::Success {
                post_state_hash,
                effects,
            } => Ok((post_state_hash, effects)),
        }
    }
}
