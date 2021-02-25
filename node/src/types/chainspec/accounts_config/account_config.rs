use datasize::DataSize;
use num::Zero;
#[cfg(test)]
use rand::{distributions::Standard, prelude::*};
use serde::{Deserialize, Serialize};

use casper_execution_engine::{core::engine_state::GenesisAccount, shared::motes::Motes};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey,
};
#[cfg(test)]
use casper_types::{SecretKey, U512};

#[cfg(test)]
use crate::testing::TestRng;

#[derive(PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Copy, Clone)]
pub struct AccountConfig {
    pub(super) public_key: PublicKey,
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

    #[cfg(test)]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let public_key = PublicKey::from(&SecretKey::ed25519(rng.gen()));
        let balance = Motes::new(U512::from(rng.gen::<u64>()));
        let bonded_amount = Motes::new(U512::from(rng.gen::<u64>()));

        AccountConfig {
            public_key,
            balance,
            bonded_amount,
        }
    }
}

#[cfg(test)]
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

impl From<AccountConfig> for GenesisAccount {
    fn from(account_config: AccountConfig) -> Self {
        GenesisAccount::account(
            account_config.public_key,
            account_config.balance,
            account_config.bonded_amount,
        )
    }
}
