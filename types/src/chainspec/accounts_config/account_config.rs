#[cfg(feature = "datasize")]
use datasize::DataSize;
use num::Zero;

#[cfg(any(feature = "testing", test))]
use rand::{distributions::Standard, prelude::*};

use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    GenesisAccount, Motes, PublicKey,
};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(any(feature = "testing", test))]
use crate::{SecretKey, U512};

use super::ValidatorConfig;

/// Configuration of an individial account in accounts.toml
#[derive(PartialEq, Ord, PartialOrd, Eq, Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct AccountConfig {
    /// Public Key.
    pub public_key: PublicKey,
    /// Balance.
    pub balance: Motes,
    /// Validator config.
    pub validator: Option<ValidatorConfig>,
}

impl AccountConfig {
    /// Creates a new `AccountConfig`.
    pub fn new(public_key: PublicKey, balance: Motes, validator: Option<ValidatorConfig>) -> Self {
        Self {
            public_key,
            balance,
            validator,
        }
    }

    /// Public key.
    pub fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    /// Balance.
    pub fn balance(&self) -> Motes {
        self.balance
    }

    /// Bonded amount.
    pub fn bonded_amount(&self) -> Motes {
        match self.validator {
            Some(validator_config) => validator_config.bonded_amount(),
            None => Motes::zero(),
        }
    }

    /// Is this a genesis validator?
    pub fn is_genesis_validator(&self) -> bool {
        self.validator.is_some()
    }

    #[cfg(any(feature = "testing", test))]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let public_key =
            PublicKey::from(&SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap());
        let balance = Motes::new(rng.gen());
        let validator = rng.gen();

        AccountConfig {
            public_key,
            balance,
            validator,
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<AccountConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AccountConfig {
        let secret_key = SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap();
        let public_key = PublicKey::from(&secret_key);

        let mut u512_array = [0u8; 64];
        rng.fill_bytes(u512_array.as_mut());
        let balance = Motes::new(U512::from(u512_array));

        let validator = rng.gen();

        AccountConfig::new(public_key, balance, validator)
    }
}

impl ToBytes for AccountConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.public_key.to_bytes()?);
        buffer.extend(self.balance.to_bytes()?);
        buffer.extend(self.validator.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.public_key.serialized_length()
            + self.balance.serialized_length()
            + self.validator.serialized_length()
    }
}

impl FromBytes for AccountConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (balance, remainder) = FromBytes::from_bytes(remainder)?;
        let (validator, remainder) = FromBytes::from_bytes(remainder)?;
        let account_config = AccountConfig {
            public_key,
            balance,
            validator,
        };
        Ok((account_config, remainder))
    }
}

impl From<AccountConfig> for GenesisAccount {
    fn from(account_config: AccountConfig) -> Self {
        let genesis_validator = account_config.validator.map(Into::into);
        GenesisAccount::account(
            account_config.public_key,
            account_config.balance,
            genesis_validator,
        )
    }
}
