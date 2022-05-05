//! The accounts config is a set of configuration options that is used to create accounts at
//! genesis, and set up auction contract with validators and delegators.
mod account_config;
mod administrator_config;
mod delegator_config;
mod validator_config;

use std::path::Path;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::GenesisAccount;
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    file_utils,
};

pub use self::administrator_config::AdministratorConfig;
use super::error::ChainspecAccountsLoadError;
use crate::utils::serde_helpers;
pub use account_config::AccountConfig;
pub use delegator_config::DelegatorConfig;
pub use validator_config::ValidatorConfig;

const CHAINSPEC_ACCOUNTS_FILENAME: &str = "accounts.toml";

#[derive(PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Clone)]
pub struct AccountsConfig {
    #[serde(deserialize_with = "serde_helpers::sorted_vec_deserializer")]
    accounts: Vec<AccountConfig>,
    #[serde(default, deserialize_with = "serde_helpers::sorted_vec_deserializer")]
    delegators: Vec<DelegatorConfig>,
    #[serde(
        default,
        deserialize_with = "serde_helpers::sorted_vec_deserializer",
        skip_serializing_if = "Vec::is_empty"
    )]
    administrators: Vec<AdministratorConfig>,
}

impl AccountsConfig {
    pub fn new(
        accounts: Vec<AccountConfig>,
        delegators: Vec<DelegatorConfig>,
        administrators: Vec<AdministratorConfig>,
    ) -> Self {
        Self {
            accounts,
            delegators,
            administrators,
        }
    }

    pub fn accounts(&self) -> &[AccountConfig] {
        &self.accounts
    }

    pub fn delegators(&self) -> &[DelegatorConfig] {
        &self.delegators
    }

    pub fn administrators(&self) -> &[AdministratorConfig] {
        &self.administrators
    }

    /// Returns `Self` and the raw bytes of the file.
    ///
    /// If the file doesn't exist, returns `Ok` with an empty `AccountsConfig` and `None` bytes.
    pub(super) fn from_dir<P: AsRef<Path>>(
        dir_path: P,
    ) -> Result<(Self, Option<Bytes>), ChainspecAccountsLoadError> {
        let accounts_path = dir_path.as_ref().join(CHAINSPEC_ACCOUNTS_FILENAME);
        if !accounts_path.is_file() {
            let config = AccountsConfig::new(Vec::new(), Vec::new(), Vec::new());
            let maybe_bytes = None;
            return Ok((config, maybe_bytes));
        }
        let bytes = file_utils::read_file(accounts_path)?;
        let config: AccountsConfig = toml::from_slice(&bytes)?;
        Ok((config, Some(Bytes::from(bytes))))
    }

    #[cfg(test)]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let alpha = AccountConfig::random(rng);
        let accounts = vec![
            alpha.clone(),
            AccountConfig::random(rng),
            AccountConfig::random(rng),
            AccountConfig::random(rng),
        ];

        let mut delegator = DelegatorConfig::random(rng);
        delegator.validator_public_key = alpha.public_key;

        let delegators = vec![delegator];

        let administrators = vec![];

        AccountsConfig {
            accounts,
            delegators,
            administrators,
        }
    }
}

impl ToBytes for AccountsConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.accounts.to_bytes()?);
        buffer.extend(self.delegators.to_bytes()?);
        buffer.extend(self.administrators.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.accounts.serialized_length()
            + self.delegators.serialized_length()
            + self.administrators.serialized_length()
    }
}

impl FromBytes for AccountsConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (accounts, remainder) = FromBytes::from_bytes(bytes)?;
        let (delegators, remainder) = FromBytes::from_bytes(remainder)?;
        let (administrators, remainder) = FromBytes::from_bytes(remainder)?;
        let accounts_config = AccountsConfig::new(accounts, delegators, administrators);
        Ok((accounts_config, remainder))
    }
}

impl From<AccountsConfig> for Vec<GenesisAccount> {
    fn from(accounts_config: AccountsConfig) -> Self {
        let mut genesis_accounts = Vec::with_capacity(accounts_config.accounts.len());
        for account_config in accounts_config.accounts {
            let genesis_account = account_config.into();
            genesis_accounts.push(genesis_account);
        }
        for delegator_config in accounts_config.delegators {
            let genesis_account = delegator_config.into();
            genesis_accounts.push(genesis_account);
        }

        for administrator_config in accounts_config.administrators {
            let administrator_account = administrator_config.into();
            genesis_accounts.push(administrator_account);
        }

        genesis_accounts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let mut rng = TestRng::new();
        let accounts_config = AccountsConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&accounts_config);
    }
}
