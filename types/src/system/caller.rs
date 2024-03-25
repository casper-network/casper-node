use alloc::vec::Vec;

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    package::PackageHash,
    AddressableEntityHash, CLType, CLTyped,
};

/// Tag representing variants of CallerTag for purposes of serialization.
#[derive(FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub enum CallerTag {
    /// Initiator tag.
    Initiator = 0,
    /// Entity tag.
    Entity,
}

/// Identity of a calling entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// Initiator (calling account)
    Initiator {
        /// The account hash of the caller
        account_hash: AccountHash,
    },
    /// Entity (smart contract / system contract)
    Entity {
        /// The package hash
        package_hash: PackageHash,
        /// The entity hash
        entity_hash: AddressableEntityHash,
    },
}

impl Caller {
    /// Creates a [`Caller::Initiator`]. This represents a call into session code, and
    /// should only ever happen once in a call stack.
    pub fn initiator(account_hash: AccountHash) -> Self {
        Caller::Initiator { account_hash }
    }

    /// Creates a [`'Caller::Entity`]. This represents a call into a contract with
    /// `EntryPointType::Called`.
    pub fn entity(package_hash: PackageHash, contract_hash: AddressableEntityHash) -> Self {
        Caller::Entity {
            package_hash,
            entity_hash: contract_hash,
        }
    }

    /// Gets the tag from self.
    pub fn tag(&self) -> CallerTag {
        match self {
            Caller::Initiator { .. } => CallerTag::Initiator,

            Caller::Entity { .. } => CallerTag::Entity,
        }
    }

    /// Gets the [`AddressableEntityHash`] for both stored session and stored contract variants.
    pub fn contract_hash(&self) -> Option<&AddressableEntityHash> {
        match self {
            Caller::Initiator { .. } => None,

            Caller::Entity {
                entity_hash: contract_hash,
                ..
            } => Some(contract_hash),
        }
    }
}

impl ToBytes for Caller {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.push(self.tag() as u8);
        match self {
            Caller::Initiator { account_hash } => result.append(&mut account_hash.to_bytes()?),

            Caller::Entity {
                package_hash,
                entity_hash: contract_hash,
            } => {
                result.append(&mut package_hash.to_bytes()?);
                result.append(&mut contract_hash.to_bytes()?);
            }
        };
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Caller::Initiator { account_hash } => account_hash.serialized_length(),
                Caller::Entity {
                    package_hash,
                    entity_hash: contract_hash,
                } => package_hash.serialized_length() + contract_hash.serialized_length(),
            }
    }
}

impl FromBytes for Caller {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        let tag = CallerTag::from_u8(tag).ok_or(bytesrepr::Error::Formatting)?;
        match tag {
            CallerTag::Initiator => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((Caller::Initiator { account_hash }, remainder))
            }
            CallerTag::Entity => {
                let (package_hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (contract_hash, remainder) = AddressableEntityHash::from_bytes(remainder)?;
                Ok((
                    Caller::Entity {
                        package_hash,
                        entity_hash: contract_hash,
                    },
                    remainder,
                ))
            }
        }
    }
}

impl CLTyped for Caller {
    fn cl_type() -> CLType {
        CLType::Any
    }
}
