use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::Serialize;

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(test)]
use casper_types::testing::TestRng;

use super::AccountsConfig;

#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Debug)]
pub struct NetworkConfig {
    /// The network name.
    pub(crate) name: String,
    /// The maximum size of an accepted network message, in bytes.
    pub(crate) maximum_net_message_size: u32,
    /// Validator accounts specified in the chainspec.
    // Note: `accounts_config` must be the last field on this struct due to issues in the TOML
    // crate - see <https://github.com/alexcrichton/toml-rs/search?q=ValueAfterTable&type=issues>.
    pub(crate) accounts_config: AccountsConfig,
}

#[cfg(test)]
impl NetworkConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let name = rng.gen::<char>().to_string();
        let accounts = vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()];
        let delegators = vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()];
        let accounts_config = AccountsConfig::new(accounts, delegators);
        let maximum_net_message_size = 4 + rng.gen_range(0..4);

        NetworkConfig {
            name,
            maximum_net_message_size,
            accounts_config,
        }
    }
}

impl ToBytes for NetworkConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.name.to_bytes()?);
        buffer.extend(self.accounts_config.to_bytes()?);
        buffer.extend(self.maximum_net_message_size.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length()
            + self.accounts_config.serialized_length()
            + self.maximum_net_message_size.serialized_length()
    }
}

impl FromBytes for NetworkConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, remainder) = String::from_bytes(bytes)?;
        let (accounts_config, remainder) = FromBytes::from_bytes(remainder)?;
        let (maximum_net_message_size, remainder) = FromBytes::from_bytes(remainder)?;
        let config = NetworkConfig {
            name,
            maximum_net_message_size,
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
