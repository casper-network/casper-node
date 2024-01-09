use casper_types::ApiError;

#[repr(u16)]
#[derive(Clone, Copy)]
pub enum NFTCoreError {
    InvalidAccount = 1,
    MissingInstaller = 2,
    InvalidInstaller = 3,
    UnexpectedKeyVariant = 4,
    FailedToGetArgBytes = 5,
    FailedToCreateDictionary = 6,
    MissingCollectionName = 7,
    InvalidCollectionName = 8,
    MissingMintingStatus = 9,
    InvalidMintingStatus = 10,
    MissingCollectionSymbol = 11,
    InvalidCollectionSymbol = 12,
    MissingTotalTokenSupply = 13,
    InvalidTotalTokenSupply = 14,
    MissingMintingMode = 15,
    InvalidMintingMode = 16,
    Phantom = 17,
    ContractAlreadyInitialized = 18,
    MissingOwnershipMode = 19,
    InvalidOwnershipMode = 20,
    MissingJsonSchema = 21,
    InvalidJsonSchema = 22,
    MissingNftKind = 23,
    InvalidNftKind = 24,
    MissingHolderMode = 25,
    InvalidHolderMode = 26,
    MissingWhitelistMode = 27,
    InvalidWhitelistMode = 28,
    MissingContractWhiteList = 29,
    InvalidContractWhitelist = 30,
    EmptyContractWhitelist = 31,
    MissingReceiptName = 32,
    InvalidReceiptName = 33,
    MissingNFTMetadataKind = 34,
    InvalidNFTMetadataKind = 35,
    MissingIdentifierMode = 36,
    InvalidIdentifierMode = 37,
    MissingMetadataMutability = 38,
    InvalidMetadataMutability = 39,
    MissingBurnMode = 40,
    InvalidBurnMode = 41,
    CannotInstallWithZeroSupply = 42,
}

impl From<NFTCoreError> for ApiError {
    fn from(e: NFTCoreError) -> Self {
        ApiError::User(e as u16)
    }
}
