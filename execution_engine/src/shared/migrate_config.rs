//! Chainspec config values pertaining to migrations.

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
use datasize::DataSize;
use rand::{distributions::Standard, prelude::Distribution, Rng};
use serde::{Deserialize, Serialize};

/// Represents config values for migration.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct MigrateConfig {
    migrations: Vec<Migration>,
}

impl ToBytes for MigrateConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.migrations.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.migrations.serialized_length()
    }
}

impl FromBytes for MigrateConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (migrations, rem) = FromBytes::from_bytes(bytes)?;
        Ok((Self { migrations }, rem))
    }
}

/// Represents config values for individual migrations with their specific parameters.s
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Migration {
    /// Migrates highest `EraInfo` to `EraSummary`.
    WriteStableEraSummaryKey {
        /// Ordinal id of this migration instance. Will be run exactly once.
        migration_id: u32,
    },
    /// Search and purge trie of `EraInfo` records.
    PurgeEraInfo {
        /// Ordinal id of this migration instance. Will be run exactly once.
        migration_id: u32,
        /// Max number of `EraInfo` objects to purge in a single block.
        batch_size: u32,
    },
}

impl Migration {
    /// Migration id.
    pub fn migration_id(&self) -> u32 {
        match self {
            Migration::WriteStableEraSummaryKey { migration_id }
            | Migration::PurgeEraInfo { migration_id, .. } => *migration_id,
        }
    }
}

impl ToBytes for Migration {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        match self {
            Migration::WriteStableEraSummaryKey { migration_id } => {
                ret.append(&mut vec![0u8]);
                ret.append(&mut migration_id.to_bytes()?);
            }
            Migration::PurgeEraInfo {
                migration_id,
                batch_size,
            } => {
                ret.append(&mut vec![1u8]);
                ret.append(&mut migration_id.to_bytes()?);
                ret.append(&mut batch_size.to_bytes()?)
            }
        }
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        match self {
            Migration::WriteStableEraSummaryKey { migration_id } => {
                migration_id.serialized_length()
            }
            Migration::PurgeEraInfo {
                migration_id,
                batch_size,
            } => migration_id.serialized_length() + batch_size.serialized_length(),
        }
    }
}

impl FromBytes for Migration {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (first, rem) = <u8 as FromBytes>::from_bytes(bytes)?;
        Ok(match first {
            0u8 => {
                let (migration_id, rem) = FromBytes::from_bytes(rem)?;
                (Migration::WriteStableEraSummaryKey { migration_id }, rem)
            }
            1u8 => {
                let (migration_id, rem) = FromBytes::from_bytes(rem)?;
                let (batch_size, rem) = FromBytes::from_bytes(rem)?;
                (
                    Migration::PurgeEraInfo {
                        migration_id,
                        batch_size,
                    },
                    rem,
                )
            }
            _ => return Err(bytesrepr::Error::Formatting),
        })
    }
}

impl Distribution<MigrateConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MigrateConfig {
        let batch_size = rng.gen_range(1..10);
        let migration_type = rng.gen_range(0..2);
        let num_actions = rng.gen_range(0..10);
        let mut migrations = vec![];
        for n in 0..num_actions {
            if migration_type % 2 == 0 {
                migrations.push(Migration::WriteStableEraSummaryKey { migration_id: n });
            } else {
                migrations.push(Migration::PurgeEraInfo {
                    migration_id: n,
                    batch_size,
                });
            }
        }
        MigrateConfig { migrations }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use super::*;
    use proptest::{num, prop_compose};

    prop_compose! {
        pub fn migration_arb()(
            migration_id in num::u32::ANY,
            batch_size in num::u32::ANY,
            migration_type in num::u8::ANY,
        ) -> Migration {
            if migration_type % 2 == 0 {
                Migration::WriteStableEraSummaryKey {
                    migration_id,
                }
            } else {
                Migration::PurgeEraInfo {
                    migration_id,
                    batch_size,
                }
            }
        }
    }
}
