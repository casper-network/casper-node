//! The accounts config is a set of configuration options that is used to create accounts at
//! genesis, and set up auction contract with validators and delegators.
mod account_config;
mod delegator_config;
mod genesis;
mod validator_config;
#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey,
};

pub use account_config::AccountConfig;
pub use delegator_config::DelegatorConfig;
pub use genesis::{GenesisAccount, GenesisValidator};
pub use validator_config::ValidatorConfig;

fn sorted_vec_deserializer<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de> + Ord,
    D: Deserializer<'de>,
{
    let mut vec = Vec::<T>::deserialize(deserializer)?;
    vec.sort_unstable();
    Ok(vec)
}

/// Configuration values associated with accounts.toml
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct AccountsConfig {
    #[serde(deserialize_with = "sorted_vec_deserializer")]
    accounts: Vec<AccountConfig>,
    #[serde(default, deserialize_with = "sorted_vec_deserializer")]
    delegators: Vec<DelegatorConfig>,
}

impl AccountsConfig {
    /// Creates a new `AccountsConfig`.
    pub fn new(accounts: Vec<AccountConfig>, delegators: Vec<DelegatorConfig>) -> Self {
        Self {
            accounts,
            delegators,
        }
    }

    /// Accounts.
    pub fn accounts(&self) -> &[AccountConfig] {
        &self.accounts
    }

    /// Delegators.
    pub fn delegators(&self) -> &[DelegatorConfig] {
        &self.delegators
    }

    /// Account.
    pub fn account(&self, public_key: &PublicKey) -> Option<&AccountConfig> {
        self.accounts
            .iter()
            .find(|account| &account.public_key == public_key)
    }

    /// Is the provided public key in the set of genesis validator public keys.
    pub fn is_genesis_validator(&self, public_key: &PublicKey) -> bool {
        match self.account(public_key) {
            None => false,
            Some(account_config) => account_config.is_genesis_validator(),
        }
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

        AccountsConfig {
            accounts,
            delegators,
        }
    }
}

impl ToBytes for AccountsConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.accounts.to_bytes()?);
        buffer.extend(self.delegators.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.accounts.serialized_length() + self.delegators.serialized_length()
    }
}

impl FromBytes for AccountsConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (accounts, remainder) = FromBytes::from_bytes(bytes)?;
        let (delegators, remainder) = FromBytes::from_bytes(remainder)?;
        let accounts_config = AccountsConfig::new(accounts, delegators);
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
