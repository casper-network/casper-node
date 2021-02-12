use std::path::Path;

use csv::ReaderBuilder;
use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey, U512,
};

use super::error::ChainspecAccountsLoadError;
#[cfg(test)]
use crate::testing::TestRng;
use crate::types::Timestamp;

const CHAINSPEC_ACCOUNTS_FILENAME: &str = "accounts.csv";

/// This parses the accounts.csv file if it exists.
///
/// Returns `Ok` if there is no such file, or if there is and it can be parsed correctly.  Returns
/// an error if the file exists but can't be parsed.
pub(super) fn parse_accounts_csv<P: AsRef<Path>>(
    dir_path: P,
) -> Result<Vec<GenesisAccount>, ChainspecAccountsLoadError> {
    #[derive(Deserialize)]
    struct ParsedAccount {
        public_key: PublicKey,
        balance: U512,
        bonded_amount: U512,
    }

    let path = dir_path.as_ref().join(CHAINSPEC_ACCOUNTS_FILENAME);
    if !path.is_file() {
        return Ok(vec![]);
    }

    let mut reader = ReaderBuilder::new().has_headers(false).from_path(&path)?;
    let mut accounts = vec![];
    for result in reader.deserialize() {
        let parsed: ParsedAccount = result?;
        let balance = Motes::new(parsed.balance);
        let bonded_amount = Motes::new(parsed.bonded_amount);

        let account = GenesisAccount::new(
            parsed.public_key,
            parsed.public_key.to_account_hash(),
            balance,
            bonded_amount,
        );
        accounts.push(account);
    }
    Ok(accounts)
}

#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Debug)]
pub struct NetworkConfig {
    /// The network name.
    pub(crate) name: String,
    /// The inception moment of the network.
    pub(crate) timestamp: Timestamp,
    /// Validator accounts specified in the chainspec.
    pub(crate) accounts: Vec<GenesisAccount>,
}

impl NetworkConfig {
    /// Returns a vector of chainspec validators' public key and their stake.
    pub fn chainspec_validator_stakes(&self) -> Vec<(PublicKey, Motes)> {
        self.accounts
            .iter()
            .filter_map(|chainspec_account| {
                if chainspec_account.is_genesis_validator() {
                    let public_key = chainspec_account
                        .public_key()
                        .expect("should have chainspec account public key");

                    Some((public_key, chainspec_account.bonded_amount()))
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
        let accounts = vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()];

        NetworkConfig {
            name,
            timestamp,
            accounts,
        }
    }
}

impl ToBytes for NetworkConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.name.to_bytes()?);
        buffer.extend(self.timestamp.to_bytes()?);
        buffer.extend(self.accounts.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length()
            + self.timestamp.serialized_length()
            + self.accounts.serialized_length()
    }
}

impl FromBytes for NetworkConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, remainder) = String::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (accounts, remainder) = Vec::<GenesisAccount>::from_bytes(remainder)?;
        let config = NetworkConfig {
            name,
            timestamp,
            accounts,
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
