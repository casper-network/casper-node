use alloc::vec::Vec;

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    package::PackageHash,
    AddressableEntityHash, CLType, CLTyped,
};

/// Tag representing variants of CallStackElement for purposes of serialization.
#[derive(FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub enum CallStackElementTag {
    /// Session tag.
    Session = 0,
    /// StoredContract tag.
    StoredContract,
}

/// Represents the origin of a sub-call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallStackElement {
    /// Session
    Session {
        /// The account hash of the caller
        account_hash: AccountHash,
    },
    // /// Effectively an EntryPointType::Session - stored access to a session.
    // StoredSession {
    //     /// The account hash of the caller
    //     account_hash: AccountHash,
    //     /// The package hash
    //     package_hash: PackageHash,
    //     /// The contract hash
    //     contract_hash: AddressableEntityHash,
    // },
    /// AddressableEntity
    AddressableEntity {
        /// The package hash
        package_hash: PackageHash,
        /// The entity hash
        entity_hash: AddressableEntityHash,
    },
}

impl CallStackElement {
    /// Creates a [`CallStackElement::Session`]. This represents a call into session code, and
    /// should only ever happen once in a call stack.
    pub fn session(account_hash: AccountHash) -> Self {
        CallStackElement::Session { account_hash }
    }

    /// Creates a [`'CallStackElement::StoredContract`]. This represents a call into a contract with
    /// `EntryPointType::Contract`.
    pub fn stored_contract(
        package_hash: PackageHash,
        contract_hash: AddressableEntityHash,
    ) -> Self {
        CallStackElement::AddressableEntity {
            package_hash,
            entity_hash: contract_hash,
        }
    }

    // /// Creates a [`'CallStackElement::StoredSession`]. This represents a call into a contract
    // with /// `EntryPointType::Session`.
    // pub fn stored_session(
    //     account_hash: AccountHash,
    //     package_hash: PackageHash,
    //     contract_hash: AddressableEntityHash,
    // ) -> Self {
    //     CallStackElement::StoredSession {
    //         account_hash,
    //         package_hash,
    //         contract_hash,
    //     }
    // }

    /// Gets the tag from self.
    pub fn tag(&self) -> CallStackElementTag {
        match self {
            CallStackElement::Session { .. } => CallStackElementTag::Session,

            CallStackElement::AddressableEntity { .. } => CallStackElementTag::StoredContract,
        }
    }

    /// Gets the [`AddressableEntityHash`] for both stored session and stored contract variants.
    pub fn contract_hash(&self) -> Option<&AddressableEntityHash> {
        match self {
            CallStackElement::Session { .. } => None,

            CallStackElement::AddressableEntity {
                entity_hash: contract_hash,
                ..
            } => Some(contract_hash),
        }
    }
}

impl ToBytes for CallStackElement {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.push(self.tag() as u8);
        match self {
            CallStackElement::Session { account_hash } => {
                result.append(&mut account_hash.to_bytes()?)
            }

            CallStackElement::AddressableEntity {
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
                CallStackElement::Session { account_hash } => account_hash.serialized_length(),
                CallStackElement::AddressableEntity {
                    package_hash,
                    entity_hash: contract_hash,
                } => package_hash.serialized_length() + contract_hash.serialized_length(),
            }
    }
}

impl FromBytes for CallStackElement {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        let tag = CallStackElementTag::from_u8(tag).ok_or(bytesrepr::Error::Formatting)?;
        match tag {
            CallStackElementTag::Session => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((CallStackElement::Session { account_hash }, remainder))
            }
            CallStackElementTag::StoredContract => {
                let (package_hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (contract_hash, remainder) = AddressableEntityHash::from_bytes(remainder)?;
                Ok((
                    CallStackElement::AddressableEntity {
                        package_hash,
                        entity_hash: contract_hash,
                    },
                    remainder,
                ))
            }
        }
    }
}

impl CLTyped for CallStackElement {
    fn cl_type() -> CLType {
        CLType::Any
    }
}
