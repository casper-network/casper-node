use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::Serialize;

use casper_execution_engine::shared::motes::Motes;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey,
};

use super::AccountsConfig;
#[cfg(test)]
use crate::testing::TestRng;
use crate::types::Timestamp;

#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Debug)]
pub struct NetworkConfig {
    /// The network name.
    pub(crate) name: String,
    /// The inception moment of the network.
    pub(crate) timestamp: Timestamp,
    /// Validator accounts specified in the chainspec.
    pub(crate) accounts_config: AccountsConfig,
}

impl NetworkConfig {
    /// Returns a vector of chainspec validators' public key and their stake.
    pub fn chainspec_validator_stakes(&self) -> Vec<(PublicKey, Motes)> {
        self.accounts_config
            .accounts()
            .iter()
            .filter_map(|account_config| {
                if account_config.is_genesis_validator() {
                    Some((account_config.public_key(), account_config.bonded_amount()))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
impl NetworkConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let name = rng.gen::<char>().to_string();
        let timestamp = Timestamp::random(rng);
        let accounts_config =
            AccountsConfig::new(vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()]);

        NetworkConfig {
            name,
            timestamp,
            accounts_config,
        }
    }
}

impl ToBytes for NetworkConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.name.to_bytes()?);
        buffer.extend(self.timestamp.to_bytes()?);
        buffer.extend(self.accounts_config.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length()
            + self.timestamp.serialized_length()
            + self.accounts_config.serialized_length()
    }
}

impl FromBytes for NetworkConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, remainder) = String::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (accounts_config, remainder) = FromBytes::from_bytes(remainder)?;
        let config = NetworkConfig {
            name,
            timestamp,
            accounts_config,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let config = NetworkConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
