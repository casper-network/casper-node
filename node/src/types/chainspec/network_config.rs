use datasize::DataSize;
use juliet::ChannelConfiguration;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(test)]
use casper_types::testing::TestRng;

use crate::components::network::PerChannel;

use super::AccountsConfig;

/// Configuration values associated with the network.
#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Debug)]
pub struct NetworkConfig {
    /// The network name.
    pub name: String,
    /// The maximum size of an accepted handshake network message, in bytes.
    pub maximum_handshake_message_size: u32,
    /// Validator accounts specified in the chainspec.
    // Note: `accounts_config` must be the last field on this struct due to issues in the TOML
    // crate - see <https://github.com/alexcrichton/toml-rs/search?q=ValueAfterTable&type=issues>.
    pub accounts_config: AccountsConfig,
    /// Low level configuration.
    pub networking_config: PerChannel<JulietConfig>,
}

/// Low-level configuration for the Juliet crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, DataSize, Serialize, Deserialize)]
pub struct JulietConfig {
    /// Sets a limit for channels.
    pub in_flight_limit: u16, // 10-50
    /// The maximum size of an accepted network message, in bytes.
    pub maximum_request_payload_size: u32, //
    /// The maximum size of an accepted network message, in bytes.
    pub maximum_response_payload_size: u32,
}

impl Default for PerChannel<JulietConfig> {
    fn default() -> Self {
        //TODO figure out the right values:
        PerChannel::all(JulietConfig {
            in_flight_limit: 25,
            maximum_request_payload_size: 24 * 1024 * 1024,
            maximum_response_payload_size: 0,
        })
    }
}

#[cfg(test)]
impl NetworkConfig {
    /// Generates a random instance for fuzzy testing using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let name = rng.gen::<char>().to_string();
        let maximum_handshake_message_size = 4 + rng.gen_range(0..4);
        let accounts_config = AccountsConfig::random(rng);
        let networking_config = PerChannel::all_with(|| JulietConfig::random(rng));

        NetworkConfig {
            name,
            maximum_handshake_message_size,
            accounts_config,
            networking_config,
        }
    }
}

impl ToBytes for NetworkConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        let Self {
            name,
            maximum_handshake_message_size,
            accounts_config,

            networking_config,
        } = self;

        buffer.extend(name.to_bytes()?);
        buffer.extend(maximum_handshake_message_size.to_bytes()?);
        buffer.extend(accounts_config.to_bytes()?);
        buffer.extend(networking_config.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            name,
            maximum_handshake_message_size,
            accounts_config,
            networking_config,
        } = self;

        name.serialized_length()
            + maximum_handshake_message_size.serialized_length()
            + accounts_config.serialized_length()
            + networking_config.serialized_length()
    }
}

impl FromBytes for NetworkConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, remainder) = String::from_bytes(bytes)?;
        let (maximum_handshake_message_size, remainder) = FromBytes::from_bytes(remainder)?;
        let (accounts_config, remainder) = FromBytes::from_bytes(remainder)?;
        let (networking_config, remainder) = FromBytes::from_bytes(remainder)?;

        let config = NetworkConfig {
            name,
            maximum_handshake_message_size,
            accounts_config,
            networking_config,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
impl JulietConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let in_flight_limit = rng.gen_range(2..50);
        let maximum_request_payload_size = rng.gen_range(1024 * 1024..24 * 1024 * 1024);
        let maximum_response_payload_size = rng.gen_range(0..32);

        Self {
            in_flight_limit,
            maximum_request_payload_size,
            maximum_response_payload_size,
        }
    }
}

impl ToBytes for JulietConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        let Self {
            in_flight_limit,
            maximum_request_payload_size,
            maximum_response_payload_size,
        } = self;

        buffer.extend(in_flight_limit.to_bytes()?);
        buffer.extend(maximum_request_payload_size.to_bytes()?);
        buffer.extend(maximum_response_payload_size.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            in_flight_limit,
            maximum_request_payload_size,
            maximum_response_payload_size,
        } = self;

        in_flight_limit.serialized_length()
            + maximum_request_payload_size.serialized_length()
            + maximum_response_payload_size.serialized_length()
    }
}

impl FromBytes for JulietConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (in_flight_limit, remainder) = FromBytes::from_bytes(bytes)?;
        let (maximum_request_payload_size, remainder) = FromBytes::from_bytes(remainder)?;
        let (maximum_response_payload_size, remainder) = FromBytes::from_bytes(remainder)?;

        let config = Self {
            in_flight_limit,
            maximum_request_payload_size,
            maximum_response_payload_size,
        };
        Ok((config, remainder))
    }
}

impl From<JulietConfig> for ChannelConfiguration {
    fn from(juliet_config: JulietConfig) -> Self {
        ChannelConfiguration::new()
            .with_request_limit(juliet_config.in_flight_limit)
            .with_max_request_payload_size(juliet_config.maximum_request_payload_size)
            .with_max_response_payload_size(juliet_config.maximum_response_payload_size)
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
