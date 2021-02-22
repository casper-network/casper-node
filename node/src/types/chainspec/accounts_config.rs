use std::path::Path;

use datasize::DataSize;
use num::Zero;
use rand::{distributions::Standard, prelude::*};
use serde::{Deserialize, Serialize};

use casper_execution_engine::{core::engine_state::GenesisAccount, shared::motes::Motes};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey, SecretKey, U512,
};

use crate::utils::{self, Loadable};

use super::error::ChainspecAccountsLoadError;

const CHAINSPEC_ACCOUNTS_FILENAME: &str = "accounts.toml";

#[derive(PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Copy, Clone)]
pub struct AccountConfig {
    public_key: PublicKey,
    balance: Motes,
    bonded_amount: Motes,
}

impl AccountConfig {
    pub fn new(public_key: PublicKey, balance: Motes, bonded_amount: Motes) -> Self {
        Self {
            public_key,
            balance,
            bonded_amount,
        }
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub fn balance(&self) -> Motes {
        self.balance
    }

    pub fn bonded_amount(&self) -> Motes {
        self.bonded_amount
    }

    pub fn is_genesis_validator(&self) -> bool {
        !self.bonded_amount.is_zero()
    }
}

impl Distribution<AccountConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AccountConfig {
        let public_key = SecretKey::ed25519(rng.gen()).into();

        let mut u512_array = [0u8; 64];
        rng.fill_bytes(u512_array.as_mut());
        let balance = Motes::new(U512::from(u512_array));

        rng.fill_bytes(u512_array.as_mut());
        let bonded_amount = Motes::new(U512::from(u512_array));

        AccountConfig::new(public_key, balance, bonded_amount)
    }
}

impl ToBytes for AccountConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.public_key.to_bytes()?);
        buffer.extend(self.balance.to_bytes()?);
        buffer.extend(self.bonded_amount.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.public_key.serialized_length()
            + self.balance.serialized_length()
            + self.bonded_amount.serialized_length()
    }
}

impl FromBytes for AccountConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (balance, remainder) = FromBytes::from_bytes(remainder)?;
        let (bonded_amount, remainder) = FromBytes::from_bytes(remainder)?;
        let account_config = AccountConfig {
            public_key,
            balance,
            bonded_amount,
        };
        Ok((account_config, remainder))
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Clone)]
pub struct AccountsConfig {
    accounts: Vec<AccountConfig>,
}

impl AccountsConfig {
    pub fn new(accounts: Vec<AccountConfig>) -> Self {
        Self { accounts }
    }

    pub fn accounts(&self) -> &[AccountConfig] {
        &self.accounts
    }
}

impl ToBytes for AccountsConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.accounts.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.accounts.serialized_length()
    }
}

impl FromBytes for AccountsConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (accounts, remainder) = FromBytes::from_bytes(bytes)?;
        let accounts_config = AccountsConfig::new(accounts);
        Ok((accounts_config, remainder))
    }
}

impl Loadable for AccountsConfig {
    type Error = ChainspecAccountsLoadError;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        let accounts_path = path.as_ref().join(CHAINSPEC_ACCOUNTS_FILENAME);
        if !accounts_path.is_file() {
            return Ok(AccountsConfig::new(vec![]));
        }
        let bytes = utils::read_file(accounts_path)?;
        let toml_chainspec: AccountsConfig = toml::from_slice(&bytes)?;
        Ok(toml_chainspec)
    }
}

impl From<AccountsConfig> for Vec<GenesisAccount> {
    fn from(accounts_config: AccountsConfig) -> Self {
        let mut genesis_accounts = Vec::with_capacity(accounts_config.accounts.len());
        for account in accounts_config.accounts {
            let genesis_account = GenesisAccount::new(
                account.public_key,
                account.public_key.to_account_hash(),
                account.balance,
                account.bonded_amount,
            );
            genesis_accounts.push(genesis_account);
        }
        genesis_accounts
    }
}
