//! Home of system contract type enum.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::Serialize;

use crate::{
    bytesrepr,
    bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    ApiError, EntryPoints,
};

const MINT_TAG: u8 = 0;
const HANDLE_PAYMENT_TAG: u8 = 1;
const STANDARD_PAYMENT_TAG: u8 = 2;
const AUCTION_TAG: u8 = 3;

use super::{
    auction::auction_entry_points, handle_payment::handle_payment_entry_points,
    mint::mint_entry_points, standard_payment::standard_payment_entry_points,
};

/// System contract types.
///
/// Used by converting to a `u32` and passing as the `system_contract_index` argument of
/// `ext_ffi::casper_get_system_contract()`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum SystemContractType {
    /// Mint contract.
    #[default]
    Mint,
    /// Handle Payment contract.
    HandlePayment,
    /// Standard Payment contract.
    StandardPayment,
    /// Auction contract.
    Auction,
}

impl ToBytes for SystemContractType {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            SystemContractType::Mint => {
                writer.push(MINT_TAG);
            }
            SystemContractType::HandlePayment => {
                writer.push(HANDLE_PAYMENT_TAG);
            }
            SystemContractType::StandardPayment => {
                writer.push(STANDARD_PAYMENT_TAG);
            }
            SystemContractType::Auction => writer.push(AUCTION_TAG),
        }
        Ok(())
    }
}

impl FromBytes for SystemContractType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MINT_TAG => Ok((SystemContractType::Mint, remainder)),
            HANDLE_PAYMENT_TAG => Ok((SystemContractType::HandlePayment, remainder)),
            STANDARD_PAYMENT_TAG => Ok((SystemContractType::StandardPayment, remainder)),
            AUCTION_TAG => Ok((SystemContractType::Auction, remainder)),
            _ => Err(Error::Formatting),
        }
    }
}

/// Name of mint system contract
pub const MINT: &str = "mint";
/// Name of handle payment system contract
pub const HANDLE_PAYMENT: &str = "handle payment";
/// Name of standard payment system contract
pub const STANDARD_PAYMENT: &str = "standard payment";
/// Name of auction system contract
pub const AUCTION: &str = "auction";

impl SystemContractType {
    /// Returns the name of the system contract.
    pub fn contract_name(&self) -> String {
        match self {
            SystemContractType::Mint => MINT.to_string(),
            SystemContractType::HandlePayment => HANDLE_PAYMENT.to_string(),
            SystemContractType::StandardPayment => STANDARD_PAYMENT.to_string(),
            SystemContractType::Auction => AUCTION.to_string(),
        }
    }

    /// Returns the entrypoint of the system contract.
    pub fn contract_entry_points(&self) -> EntryPoints {
        match self {
            SystemContractType::Mint => mint_entry_points(),
            SystemContractType::HandlePayment => handle_payment_entry_points(),
            SystemContractType::StandardPayment => standard_payment_entry_points(),
            SystemContractType::Auction => auction_entry_points(),
        }
    }
}

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
