#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_traits::Zero;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use crate::{
    account::AccountHash,
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    system::auction::DelegationRate,
    Motes, PublicKey, SecretKey,
};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

#[repr(u8)]
enum GenesisAccountTag {
    System = 0,
    Account = 1,
    Delegator = 2,
    Administrator = 3,
}

/// Represents details about genesis account's validator status.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct GenesisValidator {
    /// Stake of a genesis validator.
    bonded_amount: Motes,
    /// Delegation rate in the range of 0-100.
    delegation_rate: DelegationRate,
}

impl ToBytes for GenesisValidator {
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

impl FromBytes for GenesisValidator {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonded_amount, remainder) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, remainder) = FromBytes::from_bytes(remainder)?;
        let genesis_validator = GenesisValidator {
            bonded_amount,
            delegation_rate,
        };
        Ok((genesis_validator, remainder))
    }
}

impl GenesisValidator {
    /// Creates new [`GenesisValidator`].
    pub fn new(bonded_amount: Motes, delegation_rate: DelegationRate) -> Self {
        Self {
            bonded_amount,
            delegation_rate,
        }
    }

    /// Returns the bonded amount of a genesis validator.
    pub fn bonded_amount(&self) -> Motes {
        self.bonded_amount
    }

    /// Returns the delegation rate of a genesis validator.
    pub fn delegation_rate(&self) -> DelegationRate {
        self.delegation_rate
    }
}

impl Distribution<GenesisValidator> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisValidator {
        let bonded_amount = Motes::new(rng.gen());
        let delegation_rate = rng.gen();

        GenesisValidator::new(bonded_amount, delegation_rate)
    }
}

/// Special account in the system that is useful only for some private chains.
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct AdministratorAccount {
    public_key: PublicKey,
    balance: Motes,
}

impl AdministratorAccount {
    /// Creates new special account.
    pub fn new(public_key: PublicKey, balance: Motes) -> Self {
        Self {
            public_key,
            balance,
        }
    }

    /// Gets a reference to the administrator account's public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

impl ToBytes for AdministratorAccount {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let AdministratorAccount {
            public_key,
            balance,
        } = self;
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(public_key.to_bytes()?);
        buffer.extend(balance.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let AdministratorAccount {
            public_key,
            balance,
        } = self;
        public_key.serialized_length() + balance.serialized_length()
    }
}

impl FromBytes for AdministratorAccount {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (balance, remainder) = FromBytes::from_bytes(remainder)?;
        let administrator_account = AdministratorAccount {
            public_key,
            balance,
        };
        Ok((administrator_account, remainder))
    }
}

/// This enum represents possible states of a genesis account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum GenesisAccount {
    /// This variant is for internal use only - genesis process will create a virtual system
    /// account and use it to call system contracts.
    System,
    /// Genesis account that will be created.
    Account {
        /// Public key of a genesis account.
        public_key: PublicKey,
        /// Starting balance of a genesis account.
        balance: Motes,
        /// If set, it will make this account a genesis validator.
        validator: Option<GenesisValidator>,
    },
    /// The genesis delegator is a special account that will be created as a delegator.
    /// It does not have any stake of its own, but will create a real account in the system
    /// which will delegate to a genesis validator.
    Delegator {
        /// Validator's public key that has to refer to other instance of
        /// [`GenesisAccount::Account`] with a `validator` field set.
        validator_public_key: PublicKey,
        /// Public key of the genesis account that will be created as part of this entry.
        delegator_public_key: PublicKey,
        /// Starting balance of the account.
        balance: Motes,
        /// Delegated amount for given `validator_public_key`.
        delegated_amount: Motes,
    },
    /// An administrative account in the genesis process.
    ///
    /// This variant makes sense for some private chains.
    Administrator(AdministratorAccount),
}

impl From<AdministratorAccount> for GenesisAccount {
    fn from(v: AdministratorAccount) -> Self {
        Self::Administrator(v)
    }
}

impl GenesisAccount {
    /// Create a system account variant.
    pub fn system() -> Self {
        Self::System
    }

    /// Create a standard account variant.
    pub fn account(
        public_key: PublicKey,
        balance: Motes,
        validator: Option<GenesisValidator>,
    ) -> Self {
        Self::Account {
            public_key,
            balance,
            validator,
        }
    }

    /// Create a delegator account variant.
    pub fn delegator(
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        balance: Motes,
        delegated_amount: Motes,
    ) -> Self {
        Self::Delegator {
            validator_public_key,
            delegator_public_key,
            balance,
            delegated_amount,
        }
    }

    /// The public key (if any) associated with the account.
    pub fn public_key(&self) -> PublicKey {
        match self {
            GenesisAccount::System => PublicKey::System,
            GenesisAccount::Account { public_key, .. } => public_key.clone(),
            GenesisAccount::Delegator {
                delegator_public_key,
                ..
            } => delegator_public_key.clone(),
            GenesisAccount::Administrator(AdministratorAccount { public_key, .. }) => {
                public_key.clone()
            }
        }
    }

    /// The account hash for the account.
    pub fn account_hash(&self) -> AccountHash {
        match self {
            GenesisAccount::System => PublicKey::System.to_account_hash(),
            GenesisAccount::Account { public_key, .. } => public_key.to_account_hash(),
            GenesisAccount::Delegator {
                delegator_public_key,
                ..
            } => delegator_public_key.to_account_hash(),
            GenesisAccount::Administrator(AdministratorAccount { public_key, .. }) => {
                public_key.to_account_hash()
            }
        }
    }

    /// How many motes are to be deposited in the account's main purse.
    pub fn balance(&self) -> Motes {
        match self {
            GenesisAccount::System => Motes::zero(),
            GenesisAccount::Account { balance, .. } => *balance,
            GenesisAccount::Delegator { balance, .. } => *balance,
            GenesisAccount::Administrator(AdministratorAccount { balance, .. }) => *balance,
        }
    }

    /// How many motes are to be staked.
    ///
    /// Staked accounts are either validators with some amount of bonded stake or delgators with
    /// some amount of delegated stake.
    pub fn staked_amount(&self) -> Motes {
        match self {
            GenesisAccount::System { .. }
            | GenesisAccount::Account {
                validator: None, ..
            } => Motes::zero(),
            GenesisAccount::Account {
                validator: Some(genesis_validator),
                ..
            } => genesis_validator.bonded_amount(),
            GenesisAccount::Delegator {
                delegated_amount, ..
            } => *delegated_amount,
            GenesisAccount::Administrator(AdministratorAccount {
                public_key: _,
                balance: _,
            }) => {
                // This is defaulted to zero because administrator accounts are filtered out before
                // validator set is created at the genesis.
                Motes::zero()
            }
        }
    }

    /// What is the delegation rate of a validator.
    pub fn delegation_rate(&self) -> DelegationRate {
        match self {
            GenesisAccount::Account {
                validator: Some(genesis_validator),
                ..
            } => genesis_validator.delegation_rate(),
            GenesisAccount::System
            | GenesisAccount::Account {
                validator: None, ..
            }
            | GenesisAccount::Delegator { .. } => {
                // This value represents a delegation rate in invalid state that system is supposed
                // to reject if used.
                DelegationRate::max_value()
            }
            GenesisAccount::Administrator(AdministratorAccount { .. }) => {
                DelegationRate::max_value()
            }
        }
    }

    /// Is this a virtual system account.
    pub fn is_system_account(&self) -> bool {
        matches!(self, GenesisAccount::System { .. })
    }

    /// Is this a validator account.
    pub fn is_validator(&self) -> bool {
        match self {
            GenesisAccount::Account {
                validator: Some(_), ..
            } => true,
            GenesisAccount::System { .. }
            | GenesisAccount::Account {
                validator: None, ..
            }
            | GenesisAccount::Delegator { .. }
            | GenesisAccount::Administrator(AdministratorAccount { .. }) => false,
        }
    }

    /// Details about the genesis validator.
    pub fn validator(&self) -> Option<&GenesisValidator> {
        match self {
            GenesisAccount::Account {
                validator: Some(genesis_validator),
                ..
            } => Some(genesis_validator),
            _ => None,
        }
    }

    /// Is this a delegator account.
    pub fn is_delegator(&self) -> bool {
        matches!(self, GenesisAccount::Delegator { .. })
    }

    /// Details about the genesis delegator.
    pub fn as_delegator(&self) -> Option<(&PublicKey, &PublicKey, &Motes, &Motes)> {
        match self {
            GenesisAccount::Delegator {
                validator_public_key,
                delegator_public_key,
                balance,
                delegated_amount,
            } => Some((
                validator_public_key,
                delegator_public_key,
                balance,
                delegated_amount,
            )),
            _ => None,
        }
    }

    /// Gets the administrator account variant.
    pub fn as_administrator_account(&self) -> Option<&AdministratorAccount> {
        if let Self::Administrator(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl Distribution<GenesisAccount> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisAccount {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes[..]);
        let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
        let public_key = PublicKey::from(&secret_key);
        let balance = Motes::new(rng.gen());
        let validator = rng.gen();

        GenesisAccount::account(public_key, balance, validator)
    }
}

impl ToBytes for GenesisAccount {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            GenesisAccount::System => {
                buffer.push(GenesisAccountTag::System as u8);
            }
            GenesisAccount::Account {
                public_key,
                balance,
                validator,
            } => {
                buffer.push(GenesisAccountTag::Account as u8);
                buffer.extend(public_key.to_bytes()?);
                buffer.extend(balance.value().to_bytes()?);
                buffer.extend(validator.to_bytes()?);
            }
            GenesisAccount::Delegator {
                validator_public_key,
                delegator_public_key,
                balance,
                delegated_amount,
            } => {
                buffer.push(GenesisAccountTag::Delegator as u8);
                buffer.extend(validator_public_key.to_bytes()?);
                buffer.extend(delegator_public_key.to_bytes()?);
                buffer.extend(balance.value().to_bytes()?);
                buffer.extend(delegated_amount.value().to_bytes()?);
            }
            GenesisAccount::Administrator(administrator_account) => {
                buffer.push(GenesisAccountTag::Administrator as u8);
                buffer.extend(administrator_account.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        match self {
            GenesisAccount::System => TAG_LENGTH,
            GenesisAccount::Account {
                public_key,
                balance,
                validator,
            } => {
                public_key.serialized_length()
                    + balance.value().serialized_length()
                    + validator.serialized_length()
                    + TAG_LENGTH
            }
            GenesisAccount::Delegator {
                validator_public_key,
                delegator_public_key,
                balance,
                delegated_amount,
            } => {
                validator_public_key.serialized_length()
                    + delegator_public_key.serialized_length()
                    + balance.value().serialized_length()
                    + delegated_amount.value().serialized_length()
                    + TAG_LENGTH
            }
            GenesisAccount::Administrator(administrator_account) => {
                administrator_account.serialized_length() + TAG_LENGTH
            }
        }
    }
}

impl FromBytes for GenesisAccount {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            tag if tag == GenesisAccountTag::System as u8 => {
                let genesis_account = GenesisAccount::system();
                Ok((genesis_account, remainder))
            }
            tag if tag == GenesisAccountTag::Account as u8 => {
                let (public_key, remainder) = FromBytes::from_bytes(remainder)?;
                let (balance, remainder) = FromBytes::from_bytes(remainder)?;
                let (validator, remainder) = FromBytes::from_bytes(remainder)?;
                let genesis_account = GenesisAccount::account(public_key, balance, validator);
                Ok((genesis_account, remainder))
            }
            tag if tag == GenesisAccountTag::Delegator as u8 => {
                let (validator_public_key, remainder) = FromBytes::from_bytes(remainder)?;
                let (delegator_public_key, remainder) = FromBytes::from_bytes(remainder)?;
                let (balance, remainder) = FromBytes::from_bytes(remainder)?;
                let (delegated_amount_value, remainder) = FromBytes::from_bytes(remainder)?;
                let genesis_account = GenesisAccount::delegator(
                    validator_public_key,
                    delegator_public_key,
                    balance,
                    Motes::new(delegated_amount_value),
                );
                Ok((genesis_account, remainder))
            }
            tag if tag == GenesisAccountTag::Administrator as u8 => {
                let (administrator_account, remainder) =
                    AdministratorAccount::from_bytes(remainder)?;
                let genesis_account = GenesisAccount::Administrator(administrator_account);
                Ok((genesis_account, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
