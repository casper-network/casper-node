//! System modules, formerly known as "system contracts"
pub mod auction;
pub mod handle_payment;
pub mod mint;
pub mod standard_payment;

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

    use crate::{ApiError, ContractHash, HashAddr, U256};

    /// System contract types.
    ///
    /// Represents a specific system contract and allows easy conversion into a [`ContractHash`].
    #[derive(Debug, Copy, Clone, PartialEq)]
    #[repr(u8)]
    pub enum SystemContractType {
        /// Mint contract.
        Mint = 1,
        /// Auction contract.
        Auction = 2,
        /// Handle Payment contract.
        HandlePayment = 3,
        /// Standard Payment contract.
        StandardPayment = 4,
    }

    /// Name of mint system contract
    pub const MINT: &str = "mint";
    /// Name of auction system contract
    pub const AUCTION: &str = "auction";
    /// Name of handle payment system contract
    pub const HANDLE_PAYMENT: &str = "handle payment";
    /// Name of standard payment system contract
    pub const STANDARD_PAYMENT: &str = "standard payment";

    impl SystemContractType {
        fn into_address(self) -> HashAddr {
            let address_value = U256::from(self as u8);
            let mut address: HashAddr = Default::default();
            address_value.to_big_endian(&mut address);
            address
        }

        /// Returns a fixed [`ContractHash`] value for given contract type.
        pub fn into_contract_hash(self) -> ContractHash {
            ContractHash::new(self.into_address())
        }
    }

    impl From<SystemContractType> for u32 {
        fn from(system_contract_type: SystemContractType) -> u32 {
            system_contract_type as u32
        }
    }

    // This conversion is not intended to be used by third party crates.
    #[doc(hidden)]
    impl TryFrom<u32> for SystemContractType {
        type Error = ApiError;

        fn try_from(value: u32) -> Result<SystemContractType, Self::Error> {
            match value {
                x if x == SystemContractType::Mint as u32 => Ok(SystemContractType::Mint),
                x if x == SystemContractType::Auction as u32 => Ok(SystemContractType::Auction),
                x if x == SystemContractType::StandardPayment as u32 => {
                    Ok(SystemContractType::StandardPayment)
                }
                x if x == SystemContractType::HandlePayment as u32 => {
                    Ok(SystemContractType::HandlePayment)
                }
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

        const MINT_CONTRACT_HASH: ContractHash = ContractHash::new([
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            SystemContractType::Mint as u8,
        ]);
        const HANDLE_PAYMENT_CONTRACT_HASH: ContractHash = ContractHash::new([
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            SystemContractType::HandlePayment as u8,
        ]);
        const STANDARD_PAYMENT_CONTRACT_HASH: ContractHash = ContractHash::new([
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            SystemContractType::StandardPayment as u8,
        ]);
        const AUCTION_CONTRACT_HASH: ContractHash = ContractHash::new([
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            SystemContractType::Auction as u8,
        ]);

        #[test]
        fn get_index_of_mint_contract() {
            let index: u32 = SystemContractType::Mint.into();
            assert_eq!(index, SystemContractType::Mint as u32);
            assert_eq!(SystemContractType::Mint.to_string(), MINT);
        }

        #[test]
        fn get_index_of_handle_payment_contract() {
            let index: u32 = SystemContractType::HandlePayment.into();
            assert_eq!(index, SystemContractType::HandlePayment as u32);
            assert_eq!(
                SystemContractType::HandlePayment.to_string(),
                HANDLE_PAYMENT
            );
        }

        #[test]
        fn get_index_of_standard_payment_contract() {
            let index: u32 = SystemContractType::StandardPayment.into();
            assert_eq!(index, SystemContractType::StandardPayment as u32);
            assert_eq!(
                SystemContractType::StandardPayment.to_string(),
                STANDARD_PAYMENT
            );
        }

        #[test]
        fn get_index_of_auction_contract() {
            let index: u32 = SystemContractType::Auction.into();
            assert_eq!(index, SystemContractType::Auction as u32);
            assert_eq!(SystemContractType::Auction.to_string(), AUCTION);
        }

        #[test]
        fn create_invalid_variant_from_int() {
            assert!(SystemContractType::try_from(0).is_err());
        }

        #[test]
        fn create_mint_variant_from_int() {
            let mint = SystemContractType::try_from(1).ok().unwrap();
            assert_eq!(mint, SystemContractType::Mint);
        }

        #[test]
        fn create_auction_variant_from_int() {
            let auction = SystemContractType::try_from(2).ok().unwrap();
            assert_eq!(auction, SystemContractType::Auction);
        }

        #[test]
        fn create_handle_payment_from_int() {
            let handle_payment = SystemContractType::try_from(3).ok().unwrap();
            assert_eq!(handle_payment, SystemContractType::HandlePayment);
        }

        #[test]
        fn create_unknown_system_contract_variant() {
            assert!(SystemContractType::try_from(6).is_err());
            assert!(SystemContractType::try_from(5).is_err());
            assert!(SystemContractType::try_from(10).is_err());
            assert!(SystemContractType::try_from(u32::max_value()).is_err());
        }

        #[test]
        fn create_contract_hash_from() {
            assert_eq!(
                SystemContractType::Auction.into_contract_hash(),
                AUCTION_CONTRACT_HASH
            );
            assert_eq!(
                SystemContractType::HandlePayment.into_contract_hash(),
                HANDLE_PAYMENT_CONTRACT_HASH
            );
            assert_eq!(
                SystemContractType::Mint.into_contract_hash(),
                MINT_CONTRACT_HASH
            );
            assert_eq!(
                SystemContractType::StandardPayment.into_contract_hash(),
                STANDARD_PAYMENT_CONTRACT_HASH
            );
        }
    }
}
