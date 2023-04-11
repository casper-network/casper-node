use datasize::DataSize;
use num::Zero;
#[cfg(test)]
use rand::{distributions::Standard, prelude::*};
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::genesis::GenesisValidator;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::DelegationRate,
    Motes,
};
#[cfg(test)]
use casper_types::{testing::TestRng, U512};

#[derive(PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize, DataSize, Debug, Copy, Clone)]
pub struct ValidatorConfig {
    bonded_amount: Motes,
    #[serde(default = "DelegationRate::zero")]
    delegation_rate: DelegationRate,
}

impl ValidatorConfig {
    pub fn new(bonded_amount: Motes, delegation_rate: DelegationRate) -> Self {
        Self {
            bonded_amount,
            delegation_rate,
        }
    }

    pub fn delegation_rate(&self) -> DelegationRate {
        self.delegation_rate
    }

    pub fn bonded_amount(&self) -> Motes {
        self.bonded_amount
    }

    #[cfg(test)]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let bonded_amount = Motes::new(U512::from(rng.gen::<u64>()));
        let delegation_rate = rng.gen();

        ValidatorConfig {
            bonded_amount,
            delegation_rate,
        }
    }
}

#[cfg(test)]
impl Distribution<ValidatorConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ValidatorConfig {
        let mut u512_array = [0; 64];
        rng.fill_bytes(u512_array.as_mut());
        let bonded_amount = Motes::new(U512::from(u512_array));

        let delegation_rate = rng.gen();

        ValidatorConfig::new(bonded_amount, delegation_rate)
    }
}

impl ToBytes for ValidatorConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.bonded_amount.to_bytes()?);
        buffer.extend(self.delegation_rate.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.bonded_amount.serialized_length() + self.delegation_rate.serialized_length()
    }
}

impl FromBytes for ValidatorConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonded_amount, remainder) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, remainder) = FromBytes::from_bytes(remainder)?;
        let account_config = ValidatorConfig {
            bonded_amount,
            delegation_rate,
        };
        Ok((account_config, remainder))
    }
}

impl From<ValidatorConfig> for GenesisValidator {
    fn from(account_config: ValidatorConfig) -> Self {
        GenesisValidator::new(
            account_config.bonded_amount(),
            account_config.delegation_rate,
        )
    }
}
