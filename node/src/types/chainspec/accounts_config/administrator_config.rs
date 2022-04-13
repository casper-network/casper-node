use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::{genesis::AdministratorAccount, GenesisAccount};
use casper_types::{
    account::Weight,
    bytesrepr::{self, FromBytes, ToBytes},
    Motes, PublicKey,
};

#[derive(PartialEq, Ord, PartialOrd, Eq, Serialize, Deserialize, DataSize, Debug, Clone)]
pub struct AdministratorConfig {
    pub(super) public_key: PublicKey,
    balance: Motes,
    weight: Weight,
}

impl AdministratorConfig {
    pub fn new(public_key: PublicKey, balance: Motes, weight: Weight) -> Self {
        Self {
            public_key,
            balance,
            weight,
        }
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    pub fn balance(&self) -> Motes {
        self.balance
    }

    /// Get the administrator config's weight.
    #[must_use]
    pub fn weight(&self) -> Weight {
        self.weight
    }
}

impl ToBytes for AdministratorConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        let AdministratorConfig {
            public_key,
            balance,
            weight,
        } = self;
        buffer.extend(public_key.to_bytes()?);
        buffer.extend(balance.to_bytes()?);
        buffer.extend(weight.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let AdministratorConfig {
            public_key,
            balance,
            weight,
        } = self;
        public_key.serialized_length() + balance.serialized_length() + weight.serialized_length()
    }
}

impl FromBytes for AdministratorConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (balance, remainder) = FromBytes::from_bytes(remainder)?;
        let (weight, remainder) = FromBytes::from_bytes(remainder)?;
        let account_config = AdministratorConfig {
            public_key,
            balance,
            weight,
        };
        Ok((account_config, remainder))
    }
}

impl From<AdministratorConfig> for AdministratorAccount {
    fn from(administrator_config: AdministratorConfig) -> Self {
        let AdministratorConfig {
            public_key,
            balance,
            weight,
        } = administrator_config;
        AdministratorAccount::new(public_key, balance, weight)
    }
}

impl From<AdministratorConfig> for GenesisAccount {
    fn from(administrator_config: AdministratorConfig) -> Self {
        let admin_account = administrator_config.into();
        GenesisAccount::Administrator(admin_account)
    }
}
