use crate::bytesrepr;
use crate::bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

const ON_USE_TAG: u8 = 0;
const AT_PROTOCOL_UPGRADE_TAG: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntityMigrationMode {
    #[default]
    OnUse,
    AtProtocolUpgrade,
}

impl ToBytes for EntityMigrationMode {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            Self::OnUse => buffer.push(ON_USE_TAG),
            Self::AtProtocolUpgrade => buffer.push(AT_PROTOCOL_UPGRADE_TAG),
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
            ON_USE_TAG => Ok((EntityMigrationMode::OnUse, rem)),
            AT_PROTOCOL_UPGRADE_TAG => Ok((EntityMigrationMode::AtProtocolUpgrade, rem)),
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntityMigrationMode> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntityMigrationMode {
        match rng.gen_range(ON_USE_TAG..=AT_PROTOCOL_UPGRADE_TAG) {
            ON_USE_TAG => EntityMigrationMode::OnUse,
            AT_PROTOCOL_UPGRADE_TAG => EntityMigrationMode::AtProtocolUpgrade,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_entity_mode_migration() {
        for tag in ON_USE_TAG..=AT_PROTOCOL_UPGRADE_TAG {
            let entity_mode_migration = match tag {
                ON_USE_TAG => EntityMigrationMode::OnUse,
                AT_PROTOCOL_UPGRADE_TAG => EntityMigrationMode::AtProtocolUpgrade,
                _ => unreachable!(),
            };

            bytesrepr::test_serialization_roundtrip(&entity_mode_migration)
        }
    }
}
