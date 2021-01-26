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
    /// Proof of Stake contract.
    ProofOfStake,
    /// Standard Payment contract.
    StandardPayment,
    /// Auction contract.
    Auction,
}

/// Name of mint system contract
pub const MINT: &str = "mint";
/// Name of proof of stake system contract
pub const PROOF_OF_STAKE: &str = "proof of stake";
/// Name of standard payment system contract
pub const STANDARD_PAYMENT: &str = "standard payment";
/// Name of auction system contract
pub const AUCTION: &str = "auction";

impl From<SystemContractType> for u32 {
    fn from(system_contract_type: SystemContractType) -> u32 {
        match system_contract_type {
            SystemContractType::Mint => 0,
            SystemContractType::ProofOfStake => 1,
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
            1 => Ok(SystemContractType::ProofOfStake),
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
            SystemContractType::ProofOfStake => write!(f, "{}", PROOF_OF_STAKE),
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
    fn get_index_of_pos_contract() {
        let index: u32 = SystemContractType::ProofOfStake.into();
        assert_eq!(index, 1u32);
        assert_eq!(SystemContractType::ProofOfStake.to_string(), PROOF_OF_STAKE);
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
    fn create_pos_variant_from_int() {
        let pos = SystemContractType::try_from(1).ok().unwrap();
        assert_eq!(pos, SystemContractType::ProofOfStake);
    }

    #[test]
    fn create_standard_payment_variant_from_int() {
        let pos = SystemContractType::try_from(2).ok().unwrap();
        assert_eq!(pos, SystemContractType::StandardPayment);
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
