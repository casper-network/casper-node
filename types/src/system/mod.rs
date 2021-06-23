//! System modules, formerly known as "system contracts"
pub mod auction;
pub mod handle_payment;
pub mod mint;
pub mod standard_payment;

use alloc::vec::Vec;

use bytesrepr::U8_SERIALIZED_LENGTH;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

use crate::{
    account::AccountHash,
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    CLType, CLTyped, ContractHash, ContractPackageHash,
};
pub use error::Error;
pub use system_contract_type::{
    SystemContractType, AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
};

mod error {
    #[cfg(feature = "std")]
    use thiserror::Error;

    use crate::system::{auction, handle_payment, mint};

    /// An aggregate enum error with variants for each system contract's error.
    #[derive(Debug, Copy, Clone)]
    #[cfg_attr(feature = "std", derive(Error))]
    pub enum Error {
        /// Contains a [`mint::Error`].
        #[cfg_attr(feature = "std", error("Mint error: {}", _0))]
        Mint(mint::Error),
        /// Contains a [`handle_payment::Error`].
        #[cfg_attr(feature = "std", error("HandlePayment error: {}", _0))]
        HandlePayment(handle_payment::Error),
        /// Contains a [`auction::Error`].
        #[cfg_attr(feature = "std", error("Auction error: {}", _0))]
        Auction(auction::Error),
    }

    impl From<mint::Error> for Error {
        fn from(error: mint::Error) -> Error {
            Error::Mint(error)
        }
    }

    impl From<handle_payment::Error> for Error {
        fn from(error: handle_payment::Error) -> Error {
            Error::HandlePayment(error)
        }
    }

    impl From<auction::Error> for Error {
        fn from(error: auction::Error) -> Error {
            Error::Auction(error)
        }
    }
}

mod system_contract_type {
    //! Home of system contract type enum.

    use core::{
        convert::TryFrom,
        fmt::{self, Display, Formatter},
    };

    use crate::ApiError;

    /// System contract types.
    ///
    /// Used by converting to a `u32` and passing as the `system_contract_index` argument of
    /// `ext_ffi::casper_get_system_contract()`.
    #[derive(Debug, Copy, Clone, PartialEq)]
    pub enum SystemContractType {
        /// Mint contract.
        Mint,
        /// Handle Payment contract.
        HandlePayment,
        /// Standard Payment contract.
        StandardPayment,
        /// Auction contract.
        Auction,
    }

    /// Name of mint system contract
    pub const MINT: &str = "mint";
    /// Name of handle payment system contract
    pub const HANDLE_PAYMENT: &str = "handle payment";
    /// Name of standard payment system contract
    pub const STANDARD_PAYMENT: &str = "standard payment";
    /// Name of auction system contract
    pub const AUCTION: &str = "auction";

    impl From<SystemContractType> for u32 {
        fn from(system_contract_type: SystemContractType) -> u32 {
            match system_contract_type {
                SystemContractType::Mint => 0,
                SystemContractType::HandlePayment => 1,
                SystemContractType::StandardPayment => 2,
                SystemContractType::Auction => 3,
            }
        }
    }

    // This conversion is not intended to be used by third party crates.
    #[doc(hidden)]
    impl TryFrom<u32> for SystemContractType {
        type Error = ApiError;
        fn try_from(value: u32) -> Result<SystemContractType, Self::Error> {
            match value {
                0 => Ok(SystemContractType::Mint),
                1 => Ok(SystemContractType::HandlePayment),
                2 => Ok(SystemContractType::StandardPayment),
                3 => Ok(SystemContractType::Auction),
                _ => Err(ApiError::InvalidSystemContract),
            }
        }
    }

    impl Display for SystemContractType {
        fn fmt(&self, f: &mut Formatter) -> fmt::Result {
            match *self {
                SystemContractType::Mint => write!(f, "{}", MINT),
                SystemContractType::HandlePayment => write!(f, "{}", HANDLE_PAYMENT),
                SystemContractType::StandardPayment => write!(f, "{}", STANDARD_PAYMENT),
                SystemContractType::Auction => write!(f, "{}", AUCTION),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::string::ToString;

        use super::*;

        #[test]
        fn get_index_of_mint_contract() {
            let index: u32 = SystemContractType::Mint.into();
            assert_eq!(index, 0u32);
            assert_eq!(SystemContractType::Mint.to_string(), MINT);
        }

        #[test]
        fn get_index_of_handle_payment_contract() {
            let index: u32 = SystemContractType::HandlePayment.into();
            assert_eq!(index, 1u32);
            assert_eq!(
                SystemContractType::HandlePayment.to_string(),
                HANDLE_PAYMENT
            );
        }

        #[test]
        fn get_index_of_standard_payment_contract() {
            let index: u32 = SystemContractType::StandardPayment.into();
            assert_eq!(index, 2u32);
            assert_eq!(
                SystemContractType::StandardPayment.to_string(),
                STANDARD_PAYMENT
            );
        }

        #[test]
        fn get_index_of_auction_contract() {
            let index: u32 = SystemContractType::Auction.into();
            assert_eq!(index, 3u32);
            assert_eq!(SystemContractType::Auction.to_string(), AUCTION);
        }

        #[test]
        fn create_mint_variant_from_int() {
            let mint = SystemContractType::try_from(0).ok().unwrap();
            assert_eq!(mint, SystemContractType::Mint);
        }

        #[test]
        fn create_handle_payment_variant_from_int() {
            let handle_payment = SystemContractType::try_from(1).ok().unwrap();
            assert_eq!(handle_payment, SystemContractType::HandlePayment);
        }

        #[test]
        fn create_standard_payment_variant_from_int() {
            let handle_payment = SystemContractType::try_from(2).ok().unwrap();
            assert_eq!(handle_payment, SystemContractType::StandardPayment);
        }

        #[test]
        fn create_auction_variant_from_int() {
            let auction = SystemContractType::try_from(3).ok().unwrap();
            assert_eq!(auction, SystemContractType::Auction);
        }

        #[test]
        fn create_unknown_system_contract_variant() {
            assert!(SystemContractType::try_from(4).is_err());
            assert!(SystemContractType::try_from(5).is_err());
            assert!(SystemContractType::try_from(10).is_err());
            assert!(SystemContractType::try_from(u32::max_value()).is_err());
        }
    }
}

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
