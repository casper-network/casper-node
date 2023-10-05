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
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
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
#[derive(
    Debug, Clone, PartialEq, Eq, Default, PartialOrd, Ord, Hash, Serialize, Deserialize, Copy,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum SystemEntityType {
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

impl ToBytes for SystemEntityType {
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
            SystemEntityType::Mint => {
                writer.push(MINT_TAG);
            }
            SystemEntityType::HandlePayment => {
                writer.push(HANDLE_PAYMENT_TAG);
            }
            SystemEntityType::StandardPayment => {
                writer.push(STANDARD_PAYMENT_TAG);
            }
            SystemEntityType::Auction => writer.push(AUCTION_TAG),
        }
        Ok(())
    }
}

impl FromBytes for SystemEntityType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MINT_TAG => Ok((SystemEntityType::Mint, remainder)),
            HANDLE_PAYMENT_TAG => Ok((SystemEntityType::HandlePayment, remainder)),
            STANDARD_PAYMENT_TAG => Ok((SystemEntityType::StandardPayment, remainder)),
            AUCTION_TAG => Ok((SystemEntityType::Auction, remainder)),
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<SystemEntityType> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SystemEntityType {
        match rng.gen_range(0..=3) {
            0 => SystemEntityType::Mint,
            1 => SystemEntityType::Auction,
            2 => SystemEntityType::StandardPayment,
            3 => SystemEntityType::HandlePayment,
            _ => unreachable!(),
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

impl SystemEntityType {
    /// Returns the name of the system contract.
    pub fn contract_name(&self) -> String {
        match self {
            SystemEntityType::Mint => MINT.to_string(),
            SystemEntityType::HandlePayment => HANDLE_PAYMENT.to_string(),
            SystemEntityType::StandardPayment => STANDARD_PAYMENT.to_string(),
            SystemEntityType::Auction => AUCTION.to_string(),
        }
    }

    /// Returns the entrypoint of the system contract.
    pub fn contract_entry_points(&self) -> EntryPoints {
        match self {
            SystemEntityType::Mint => mint_entry_points(),
            SystemEntityType::HandlePayment => handle_payment_entry_points(),
            SystemEntityType::StandardPayment => standard_payment_entry_points(),
            SystemEntityType::Auction => auction_entry_points(),
        }
    }
}

impl From<SystemEntityType> for u32 {
    fn from(system_contract_type: SystemEntityType) -> u32 {
        match system_contract_type {
            SystemEntityType::Mint => 0,
            SystemEntityType::HandlePayment => 1,
            SystemEntityType::StandardPayment => 2,
            SystemEntityType::Auction => 3,
        }
    }
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<u32> for SystemEntityType {
    type Error = ApiError;
    fn try_from(value: u32) -> Result<SystemEntityType, Self::Error> {
        match value {
            0 => Ok(SystemEntityType::Mint),
            1 => Ok(SystemEntityType::HandlePayment),
            2 => Ok(SystemEntityType::StandardPayment),
            3 => Ok(SystemEntityType::Auction),
            _ => Err(ApiError::InvalidSystemContract),
        }
    }
}

impl Display for SystemEntityType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match *self {
            SystemEntityType::Mint => write!(f, "{}", MINT),
            SystemEntityType::HandlePayment => write!(f, "{}", HANDLE_PAYMENT),
            SystemEntityType::StandardPayment => write!(f, "{}", STANDARD_PAYMENT),
            SystemEntityType::Auction => write!(f, "{}", AUCTION),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::string::ToString;

    use super::*;

    #[test]
    fn get_index_of_mint_contract() {
        let index: u32 = SystemEntityType::Mint.into();
        assert_eq!(index, 0u32);
        assert_eq!(SystemEntityType::Mint.to_string(), MINT);
    }

    #[test]
    fn get_index_of_handle_payment_contract() {
        let index: u32 = SystemEntityType::HandlePayment.into();
        assert_eq!(index, 1u32);
        assert_eq!(SystemEntityType::HandlePayment.to_string(), HANDLE_PAYMENT);
    }

    #[test]
    fn get_index_of_standard_payment_contract() {
        let index: u32 = SystemEntityType::StandardPayment.into();
        assert_eq!(index, 2u32);
        assert_eq!(
            SystemEntityType::StandardPayment.to_string(),
            STANDARD_PAYMENT
        );
    }

    #[test]
    fn get_index_of_auction_contract() {
        let index: u32 = SystemEntityType::Auction.into();
        assert_eq!(index, 3u32);
        assert_eq!(SystemEntityType::Auction.to_string(), AUCTION);
    }

    #[test]
    fn create_mint_variant_from_int() {
        let mint = SystemEntityType::try_from(0).ok().unwrap();
        assert_eq!(mint, SystemEntityType::Mint);
    }

    #[test]
    fn create_handle_payment_variant_from_int() {
        let handle_payment = SystemEntityType::try_from(1).ok().unwrap();
        assert_eq!(handle_payment, SystemEntityType::HandlePayment);
    }

    #[test]
    fn create_standard_payment_variant_from_int() {
        let handle_payment = SystemEntityType::try_from(2).ok().unwrap();
        assert_eq!(handle_payment, SystemEntityType::StandardPayment);
    }

    #[test]
    fn create_auction_variant_from_int() {
        let auction = SystemEntityType::try_from(3).ok().unwrap();
        assert_eq!(auction, SystemEntityType::Auction);
    }

    #[test]
    fn create_unknown_system_contract_variant() {
        assert!(SystemEntityType::try_from(4).is_err());
        assert!(SystemEntityType::try_from(5).is_err());
        assert!(SystemEntityType::try_from(10).is_err());
        assert!(SystemEntityType::try_from(u32::max_value()).is_err());
    }
}
