use core::convert::TryFrom;

use crate::NFTCoreError;

#[repr(u8)]
#[derive(PartialEq)]
pub enum WhitelistMode {
    Unlocked = 0,
    Locked = 1,
}

impl TryFrom<u8> for WhitelistMode {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(WhitelistMode::Unlocked),
            1 => Ok(WhitelistMode::Locked),
            _ => Err(NFTCoreError::InvalidWhitelistMode),
        }
    }
}

#[repr(u8)]
#[derive(PartialEq, Clone, Copy)]
pub enum NFTHolderMode {
    Accounts = 0,
    Contracts = 1,
    Mixed = 2,
}

impl TryFrom<u8> for NFTHolderMode {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(NFTHolderMode::Accounts),
            1 => Ok(NFTHolderMode::Contracts),
            2 => Ok(NFTHolderMode::Mixed),
            _ => Err(NFTCoreError::InvalidHolderMode),
        }
    }
}

#[repr(u8)]
pub enum MintingMode {
    /// The ability to mint NFTs is restricted to the installing account only.
    Installer = 0,
    /// The ability to mint NFTs is not restricted.
    Public = 1,
}

impl TryFrom<u8> for MintingMode {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MintingMode::Installer),
            1 => Ok(MintingMode::Public),
            _ => Err(NFTCoreError::InvalidMintingMode),
        }
    }
}

#[repr(u8)]
pub enum NFTKind {
    /// The NFT represents a real-world physical
    /// like a house.
    Physical = 0,
    /// The NFT represents a digital asset like a unique
    /// JPEG or digital art.
    Digital = 1,
    /// The NFT is the virtual representation
    /// of a physical notion, e.g a patent
    /// or copyright.
    Virtual = 2,
}

impl TryFrom<u8> for NFTKind {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(NFTKind::Physical),
            1 => Ok(NFTKind::Digital),
            2 => Ok(NFTKind::Virtual),
            _ => Err(NFTCoreError::InvalidNftKind),
        }
    }
}

#[repr(u8)]
pub enum NFTMetadataKind {
    CEP78 = 0,
    NFT721 = 1,
    Raw = 2,
    CustomValidated = 3,
}

impl TryFrom<u8> for NFTMetadataKind {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(NFTMetadataKind::CEP78),
            1 => Ok(NFTMetadataKind::NFT721),
            2 => Ok(NFTMetadataKind::Raw),
            3 => Ok(NFTMetadataKind::CustomValidated),
            _ => Err(NFTCoreError::InvalidNFTMetadataKind),
        }
    }
}

#[repr(u8)]
pub enum OwnershipMode {
    /// The minter owns it and can never transfer it.
    Minter = 0,
    /// The minter assigns it to an address and can never be transferred.
    Assigned = 1,
    /// The NFT can be transferred even to an recipient that does not exist.
    Transferable = 2,
}

impl TryFrom<u8> for OwnershipMode {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(OwnershipMode::Minter),
            1 => Ok(OwnershipMode::Assigned),
            2 => Ok(OwnershipMode::Transferable),
            _ => Err(NFTCoreError::InvalidOwnershipMode),
        }
    }
}

#[repr(u8)]
#[derive(PartialEq)]
pub enum NFTIdentifierMode {
    Ordinal = 0,
    Hash = 1,
}

impl TryFrom<u8> for NFTIdentifierMode {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(NFTIdentifierMode::Ordinal),
            1 => Ok(NFTIdentifierMode::Hash),
            _ => Err(NFTCoreError::InvalidIdentifierMode),
        }
    }
}

#[repr(u8)]
pub enum MetadataMutability {
    Immutable = 0,
    Mutable = 1,
}

impl TryFrom<u8> for MetadataMutability {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MetadataMutability::Immutable),
            1 => Ok(MetadataMutability::Mutable),
            _ => Err(NFTCoreError::InvalidMetadataMutability),
        }
    }
}

#[repr(u8)]
pub enum BurnMode {
    Burnable = 0,
    NonBurnable = 1,
}

impl TryFrom<u8> for BurnMode {
    type Error = NFTCoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BurnMode::Burnable),
            1 => Ok(BurnMode::NonBurnable),
            _ => Err(NFTCoreError::InvalidBurnMode),
        }
    }
}
