use alloc::vec::Vec;

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped, ContractHash, ContractPackageHash,
};

/// Tag representing variants of CallStackElement for purposes of serialization.
#[derive(FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub enum CallStackElementTag {
    /// Session tag.
    Session = 0,
    /// StoredSession tag.
    StoredSession,
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
    /// Effectively an EntryPointType::Session - stored access to a session.
    StoredSession {
        /// The account hash of the caller
        account_hash: AccountHash,
        /// The contract package hash
        contract_package_hash: ContractPackageHash,
        /// The contract hash
        contract_hash: ContractHash,
    },
    /// Contract
    StoredContract {
        /// The contract package hash
        contract_package_hash: ContractPackageHash,
        /// The contract hash
        contract_hash: ContractHash,
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
        contract_package_hash: ContractPackageHash,
        contract_hash: ContractHash,
    ) -> Self {
        CallStackElement::StoredContract {
            contract_package_hash,
            contract_hash,
        }
    }

    /// Creates a [`'CallStackElement::StoredSession`]. This represents a call into a contract with
    /// `EntryPointType::Session`.
    pub fn stored_session(
        account_hash: AccountHash,
        contract_package_hash: ContractPackageHash,
        contract_hash: ContractHash,
    ) -> Self {
        CallStackElement::StoredSession {
            account_hash,
            contract_package_hash,
            contract_hash,
        }
    }

    /// Gets the tag from self.
    pub fn tag(&self) -> CallStackElementTag {
        match self {
            CallStackElement::Session { .. } => CallStackElementTag::Session,
            CallStackElement::StoredSession { .. } => CallStackElementTag::StoredSession,
            CallStackElement::StoredContract { .. } => CallStackElementTag::StoredContract,
        }
    }

    /// Gets the [`ContractHash`] for both stored session and stored contract variants.
    pub fn contract_hash(&self) -> Option<&ContractHash> {
        match self {
            CallStackElement::Session { .. } => None,
            CallStackElement::StoredSession { contract_hash, .. }
            | CallStackElement::StoredContract { contract_hash, .. } => Some(contract_hash),
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
            CallStackElement::StoredSession {
                account_hash,
                contract_package_hash,
                contract_hash,
            } => {
                result.append(&mut account_hash.to_bytes()?);
                result.append(&mut contract_package_hash.to_bytes()?);
                result.append(&mut contract_hash.to_bytes()?);
            }
            CallStackElement::StoredContract {
                contract_package_hash,
                contract_hash,
            } => {
                result.append(&mut contract_package_hash.to_bytes()?);
                result.append(&mut contract_hash.to_bytes()?);
            }
        };
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                CallStackElement::Session { account_hash } => account_hash.serialized_length(),
                CallStackElement::StoredSession {
                    account_hash,
                    contract_package_hash,
                    contract_hash,
                } => {
                    account_hash.serialized_length()
                        + contract_package_hash.serialized_length()
                        + contract_hash.serialized_length()
                }
                CallStackElement::StoredContract {
                    contract_package_hash,
                    contract_hash,
                } => contract_package_hash.serialized_length() + contract_hash.serialized_length(),
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
            CallStackElementTag::StoredSession => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                let (contract_package_hash, remainder) =
                    ContractPackageHash::from_bytes(remainder)?;
                let (contract_hash, remainder) = ContractHash::from_bytes(remainder)?;
                Ok((
                    CallStackElement::StoredSession {
                        account_hash,
                        contract_package_hash,
                        contract_hash,
                    },
                    remainder,
                ))
            }
            CallStackElementTag::StoredContract => {
                let (contract_package_hash, remainder) =
                    ContractPackageHash::from_bytes(remainder)?;
                let (contract_hash, remainder) = ContractHash::from_bytes(remainder)?;
                Ok((
                    CallStackElement::StoredContract {
                        contract_package_hash,
                        contract_hash,
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
