use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, CasperABI};
use casper_sdk::types::Address;

use crate::error::NFTCoreError;

const MAX_TOTAL_TOKEN_SUPPLY: u64 = 100_000_000;

#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
pub struct CEP78State {
    pub collection_name: String,
    pub collection_symbol: String,
    pub total_token_supply: u64,
    pub allow_minting: bool,
    pub minting_mode: MintingMode,
    pub ownership_mode: OwnershipMode,
    pub nft_kind: NFTKind,
    pub whitelist_mode: WhitelistMode,
    pub acl_whitelist: Vec<Address>,
    pub acl_package_mode: bool,
    pub package_operator_mode: bool,
    pub package_hash: String,
    pub base_metadata_kind: NFTMetadataKind,
    pub optional_metadata: Vec<u8>,
    pub additional_required_metadata: Vec<u8>,
    pub json_schema: String,
    pub identifier_mode: NFTIdentifierMode,
    pub metadata_muitability: MetadataMutability,
    pub burn_mode: BurnMode,
    pub operator_burn_mode: bool,
    pub reporting_mode: OwnerReverseLookupMode,
    pub transfer_filter_contract_contract_hash: Option<Address>,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum NFTIdentifierMode {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum MetadataMutability {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum BurnMode {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum OwnerReverseLookupMode {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum NFTMetadataKind {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum MintingMode {
    Foo,
}

#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum OwnershipMode {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum NFTKind {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum NFTHolderMode {
    Foo,
}
#[derive(BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone)]
enum WhitelistMode {
    Foo,
}

impl CEP78State {
    pub fn new(
        collection_name: String,
        collection_symbol: String,
        total_token_supply: u64,
        allow_minting: bool,
        minting_mode: MintingMode,
        ownership_mode: OwnershipMode,
        nft_kind: NFTKind,
        whitelist_mode: WhitelistMode,
        acl_whitelist: Vec<Address>,
        acl_package_mode: bool,
        package_operator_mode: bool,
        package_hash: String,
        base_metadata_kind: NFTMetadataKind,
        optional_metadata: Vec<u8>,
        additional_required_metadata: Vec<u8>,
        json_schema: String,
        identifier_mode: NFTIdentifierMode,
        metadata_muitability: MetadataMutability,
        burn_mode: BurnMode,
        operator_burn_mode: bool,
        reporting_mode: OwnerReverseLookupMode,
        transfer_filter_contract_contract_hash: Option<Address>,
    ) -> Result<Self, NFTCoreError> {
        if total_token_supply > MAX_TOTAL_TOKEN_SUPPLY {
            return Err(NFTCoreError::ExceededMaxTotalSupply);
        }
        Ok(Self {
            collection_name,
            collection_symbol,
            total_token_supply,
            allow_minting,
            minting_mode,
            ownership_mode,
            nft_kind,
            whitelist_mode,
            acl_whitelist,
            acl_package_mode,
            package_operator_mode,
            package_hash,
            base_metadata_kind,
            optional_metadata,
            additional_required_metadata,
            json_schema,
            identifier_mode,
            metadata_muitability,
            burn_mode,
            operator_burn_mode,
            reporting_mode,
            transfer_filter_contract_contract_hash,
        })
    }
}

#[casper]
pub trait CEP18 {
    #[casper(private)]
    fn state(&self) -> &CEP78State;

    #[casper(private)]
    fn state_mut(&mut self) -> &mut CEP78State;
}
