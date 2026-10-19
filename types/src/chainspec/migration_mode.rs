use crate::bytesrepr;
use crate::bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

const NO_MIGRATION_TAG: u8 = 0;

const ACCOUNTS_TAG: u8 = 1;

const CONTRACTS_TAG: u8 = 2;

const ACCOUNTS_AND_CONTRACTS_TAG: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntityMigrationMode {
    #[default]
    NoMigration,
    Accounts,
    Contracts,
    AccountsAndContracts,
}

impl ToBytes for EntityMigrationMode {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            Self::NoMigration => buffer.push(NO_MIGRATION_TAG),
            Self::Accounts => buffer.push(ACCOUNTS_TAG),
            Self::Contracts => buffer.push(CONTRACTS_TAG),
            Self::AccountsAndContracts => buffer.push(ACCOUNTS_AND_CONTRACTS_TAG),
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for EntityMigrationMode {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;

        match tag {
            NO_MIGRATION_TAG => Ok((EntityMigrationMode::NoMigration, rem)),
            ACCOUNTS_TAG => Ok((EntityMigrationMode::Accounts, rem)),
            CONTRACTS_TAG => Ok((EntityMigrationMode::Contracts, rem)),
            ACCOUNTS_AND_CONTRACTS_TAG => Ok((EntityMigrationMode::AccountsAndContracts, rem)),
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntityMigrationMode> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntityMigrationMode {
        match rng.gen_range(NO_MIGRATION_TAG..=ACCOUNTS_AND_CONTRACTS_TAG) {
            NO_MIGRATION_TAG => EntityMigrationMode::NoMigration,
            ACCOUNTS_TAG => EntityMigrationMode::Accounts,
            CONTRACTS_TAG => EntityMigrationMode::Contracts,
            ACCOUNTS_AND_CONTRACTS_TAG => EntityMigrationMode::AccountsAndContracts,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_entity_mode_migration() {
        for tag in NO_MIGRATION_TAG..=ACCOUNTS_AND_CONTRACTS_TAG {
            let entity_mode_migration = match tag {
                NO_MIGRATION_TAG => EntityMigrationMode::NoMigration,
                ACCOUNTS_TAG => EntityMigrationMode::Accounts,
                CONTRACTS_TAG => EntityMigrationMode::Contracts,
                ACCOUNTS_AND_CONTRACTS_TAG => EntityMigrationMode::AccountsAndContracts,
                _ => unreachable!(),
            };

            bytesrepr::test_serialization_roundtrip(&entity_mode_migration)
        }
    }
}
