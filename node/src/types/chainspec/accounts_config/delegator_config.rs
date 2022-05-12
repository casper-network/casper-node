use datasize::DataSize;
#[cfg(test)]
use rand::{distributions::Standard, prelude::*};
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::GenesisAccount;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    Motes, PublicKey,
};
#[cfg(test)]
use casper_types::{testing::TestRng, SecretKey, U512};

#[derive(PartialEq, Ord, PartialOrd, Eq, Serialize, Deserialize, DataSize, Debug, Clone)]
pub struct DelegatorConfig {
    pub(super) validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
    balance: Motes,
    delegated_amount: Motes,
}

impl DelegatorConfig {
    pub fn new(
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        balance: Motes,
        delegated_amount: Motes,
    ) -> Self {
        Self {
            validator_public_key,
            delegator_public_key,
            balance,
            delegated_amount,
        }
    }

    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    pub fn delegator_public_key(&self) -> &PublicKey {
        &self.delegator_public_key
    }

    pub fn balance(&self) -> Motes {
        self.balance
    }

    pub fn delegated_amount(&self) -> Motes {
        self.delegated_amount
    }

    #[cfg(test)]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let validator_public_key =
            PublicKey::from(&SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap());
        let delegator_public_key =
            PublicKey::from(&SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap());
        let balance = Motes::new(U512::from(rng.gen::<u64>()));
        let delegated_amount = Motes::new(U512::from(rng.gen::<u64>()));

        DelegatorConfig {
            validator_public_key,
            delegator_public_key,
            balance,
            delegated_amount,
        }
    }
}

#[cfg(test)]
impl Distribution<DelegatorConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> DelegatorConfig {
        let validator_secret_key = SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap();
        let delegator_secret_key = SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap();

        let validator_public_key = PublicKey::from(&validator_secret_key);
        let delegator_public_key = PublicKey::from(&delegator_secret_key);

        let mut u512_array = [0u8; 64];
        rng.fill_bytes(u512_array.as_mut());
        let balance = Motes::new(U512::from(u512_array));

        rng.fill_bytes(u512_array.as_mut());
        let delegated_amount = Motes::new(U512::from(u512_array));

        DelegatorConfig::new(
            validator_public_key,
            delegator_public_key,
            balance,
            delegated_amount,
        )
    }
}

impl ToBytes for DelegatorConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.validator_public_key.to_bytes()?);
        buffer.extend(self.delegator_public_key.to_bytes()?);
        buffer.extend(self.balance.to_bytes()?);
        buffer.extend(self.delegated_amount.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.validator_public_key.serialized_length()
            + self.delegator_public_key.serialized_length()
            + self.balance.serialized_length()
            + self.delegated_amount.serialized_length()
    }
}

impl FromBytes for DelegatorConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (validator_public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (delegator_public_key, remainder) = FromBytes::from_bytes(remainder)?;
        let (balance, remainder) = FromBytes::from_bytes(remainder)?;
        let (delegated_amount, remainder) = FromBytes::from_bytes(remainder)?;
        let delegator_config = DelegatorConfig {
            validator_public_key,
            delegator_public_key,
            balance,
            delegated_amount,
        };
        Ok((delegator_config, remainder))
    }
}

impl From<DelegatorConfig> for GenesisAccount {
    fn from(delegator_config: DelegatorConfig) -> Self {
        GenesisAccount::delegator(
            delegator_config.validator_public_key,
            delegator_config.delegator_public_key,
            delegator_config.balance,
            delegator_config.delegated_amount,
        )
    }
}
