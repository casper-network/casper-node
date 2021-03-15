//! System modules, formerly known as "system contracts"
pub mod auction;
pub mod handle_payment;
pub mod mint;
pub mod standard_payment;

pub use error::Error;
pub use system_contract_type::{
    SystemContractType, AUCTION, HANDLE_PAYMENT, MINT, RESERVED_SYSTEM_CONTRACTS, STANDARD_PAYMENT,
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
        convert::{TryFrom, TryInto},
        fmt::{self, Display, Formatter},
    };

    use crate::{ApiError, ContractHash, HashAddr, Key, U256};

    pub const MINT_CONTRACT_RESERVED_ID: u8 = 1;
    pub const AUCTION_CONTRACT_RESERVED_ID: u8 = 2;
    pub const HANDLE_PAYMENT_CONTRACT_RESERVED_ID: u8 = 3;
    pub const STANDARD_PAYMENT_CONTRACT_RESERVED_ID: u8 = 4;

    /// System contract types.
    ///
    /// Represents a system contract and allows easy conversion from and into a [`ContractHash`].
    #[derive(Debug, Copy, Clone, PartialEq)]
    pub enum SystemContractType {
        /// Mint contract.
        Mint,
        /// Auction contract.
        Auction,
        /// Handle Payment contract.
        HandlePayment,
        /// Standard Payment contract.
        StandardPayment,
    }

    /// List of the reserved system contracts.
    pub const RESERVED_SYSTEM_CONTRACTS: [SystemContractType; 4] = [
        SystemContractType::Mint,
        SystemContractType::Auction,
        SystemContractType::HandlePayment,
        SystemContractType::StandardPayment,
    ];

    /// Name of mint system contract
    pub const MINT: &str = "mint";
    /// Name of auction system contract
    pub const AUCTION: &str = "auction";
    /// Name of handle payment system contract
    pub const HANDLE_PAYMENT: &str = "handle payment";
    /// Name of standard payment system contract
    pub const STANDARD_PAYMENT: &str = "standard payment";

    impl SystemContractType {
        fn reserved_id(&self) -> u8 {
            match self {
                SystemContractType::Mint => MINT_CONTRACT_RESERVED_ID,
                SystemContractType::Auction => AUCTION_CONTRACT_RESERVED_ID,
                SystemContractType::HandlePayment => HANDLE_PAYMENT_CONTRACT_RESERVED_ID,
                SystemContractType::StandardPayment => STANDARD_PAYMENT_CONTRACT_RESERVED_ID,
            }
        }

        fn into_address(self) -> HashAddr {
            let reserved_value = U256::from(self.reserved_id());
            let mut address = HashAddr::default();
            reserved_value.to_big_endian(&mut address);
            address
        }

        /// Creates a [`SystemContractType`] from an address.
        pub fn from_address(address: HashAddr) -> Option<SystemContractType> {
            // Treats a contract hash that contains as single left padded u8 value as a number.
            let address_value = U256::from_big_endian(&address);

            // Value of 0 is currently considered a special address that is not used for contract
            // storage.
            if address_value.is_zero() {
                return None;
            }

            // Limits the address space to a maximum of 255 reserved contracts.
            let contract_index: u8 = address_value.try_into().ok()?;

            match contract_index {
                MINT_CONTRACT_RESERVED_ID => Some(SystemContractType::Mint),
                AUCTION_CONTRACT_RESERVED_ID => Some(SystemContractType::Auction),
                HANDLE_PAYMENT_CONTRACT_RESERVED_ID => Some(SystemContractType::HandlePayment),
                STANDARD_PAYMENT_CONTRACT_RESERVED_ID => Some(SystemContractType::StandardPayment),
                _ => None,
            }
        }

        /// Returns a [`Key::Hash`] variant using that contains a reserved system contract hash.
        pub fn into_key(self) -> Key {
            Key::Hash(self.into_address())
        }

        /// Returns a reserved [`ContractHash`] value for given contract type.
        ///
        /// Reserved system contract hashes are in range of [1..255] and are left padded with zeros.
        pub fn into_contract_hash(self) -> ContractHash {
            let address = self.into_address();
            ContractHash::new(address)
        }

        /// Returns a name.
        pub fn name(&self) -> &str {
            match self {
                SystemContractType::Mint => MINT,
                SystemContractType::Auction => AUCTION,
                SystemContractType::HandlePayment => HANDLE_PAYMENT,
                SystemContractType::StandardPayment => STANDARD_PAYMENT,
            }
        }
    }

    impl TryFrom<ContractHash> for SystemContractType {
        type Error = ApiError;

        fn try_from(contract_hash: ContractHash) -> Result<SystemContractType, Self::Error> {
            SystemContractType::from_address(contract_hash.value())
                .ok_or(ApiError::InvalidSystemContract)
        }
    }

    impl Display for SystemContractType {
        fn fmt(&self, f: &mut Formatter) -> fmt::Result {
            write!(f, "{}", self.name())
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
            MINT_CONTRACT_RESERVED_ID,
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
            AUCTION_CONTRACT_RESERVED_ID,
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
            HANDLE_PAYMENT_CONTRACT_RESERVED_ID,
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
            STANDARD_PAYMENT_CONTRACT_RESERVED_ID,
        ]);

        fn contract_hash_from_number(value: U256) -> ContractHash {
            let mut addr: HashAddr = Default::default();
            value.to_big_endian(&mut addr);
            ContractHash::new(addr)
        }

        #[test]
        fn get_name_of_mint_contract() {
            assert_eq!(SystemContractType::Mint.to_string(), MINT);
        }

        #[test]
        fn get_name_of_handle_payment_contract() {
            assert_eq!(
                SystemContractType::HandlePayment.to_string(),
                HANDLE_PAYMENT
            );
        }

        #[test]
        fn get_name_of_standard_payment_contract() {
            assert_eq!(
                SystemContractType::StandardPayment.to_string(),
                STANDARD_PAYMENT
            );
        }

        #[test]
        fn get_name_of_auction_contract() {
            assert_eq!(SystemContractType::Auction.to_string(), AUCTION);
        }

        #[test]
        fn create_invalid_system_contract_type() {
            assert_eq!(
                SystemContractType::try_from(ContractHash::new([0xff; 32])),
                Err(ApiError::InvalidSystemContract)
            );
        }

        #[test]
        fn create_mint_variant_from_int() {
            let mint = SystemContractType::try_from(MINT_CONTRACT_HASH)
                .ok()
                .unwrap();
            assert_eq!(mint, SystemContractType::Mint);
        }

        #[test]
        fn create_handle_payment_variant_from_int() {
            let handle_payment = SystemContractType::try_from(HANDLE_PAYMENT_CONTRACT_HASH)
                .ok()
                .unwrap();
            assert_eq!(handle_payment, SystemContractType::HandlePayment);
        }

        #[test]
        fn create_standard_payment_variant_from_int() {
            let handle_payment = SystemContractType::try_from(STANDARD_PAYMENT_CONTRACT_HASH)
                .ok()
                .unwrap();
            assert_eq!(handle_payment, SystemContractType::StandardPayment);
        }

        #[test]
        fn create_auction_variant_from_int() {
            let auction = SystemContractType::try_from(AUCTION_CONTRACT_HASH)
                .ok()
                .unwrap();
            assert_eq!(auction, SystemContractType::Auction);
        }

        #[test]
        fn create_unknown_system_contract_variant() {
            assert!(SystemContractType::try_from(contract_hash_from_number(0.into())).is_err());
            assert!(SystemContractType::try_from(contract_hash_from_number(5.into())).is_err());
            assert!(SystemContractType::try_from(contract_hash_from_number(6.into())).is_err());
            assert!(SystemContractType::try_from(contract_hash_from_number(11.into())).is_err());
            assert!(SystemContractType::try_from(contract_hash_from_number(
                (u8::max_value() as u16 + 1).into()
            ))
            .is_err());
            assert!(SystemContractType::try_from(contract_hash_from_number(
                u32::max_value().into()
            ))
            .is_err());
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

        #[test]
        fn create_hash_key_from() {
            assert_eq!(
                SystemContractType::Auction.into_key(),
                Key::Hash(AUCTION_CONTRACT_HASH.value()),
            );
            assert_eq!(
                SystemContractType::HandlePayment.into_key(),
                Key::Hash(HANDLE_PAYMENT_CONTRACT_HASH.value()),
            );
            assert_eq!(
                SystemContractType::Mint.into_key(),
                Key::Hash(MINT_CONTRACT_HASH.value()),
            );
            assert_eq!(
                SystemContractType::StandardPayment.into_key(),
                Key::Hash(STANDARD_PAYMENT_CONTRACT_HASH.value()),
            );
        }
    }
}
