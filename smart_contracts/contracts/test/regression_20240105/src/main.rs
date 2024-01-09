#![no_std]
#![no_main]

mod constants;
mod error;
mod modalities;
mod utils;

extern crate alloc;

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::convert::TryInto;

use casper_types::{
    contracts::NamedKeys, runtime_args, CLType, ContractHash, ContractPackageHash, ContractVersion,
    EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter, RuntimeArgs,
};

use casper_contract::{
    contract_api::{
        runtime,
        storage::{self},
    },
    unwrap_or_revert::UnwrapOrRevert,
};

use crate::{
    constants::{
        ACCESS_KEY_NAME, ALLOW_MINTING, ARG_ALLOW_MINTING, ARG_BURN_MODE, ARG_COLLECTION_NAME,
        ARG_COLLECTION_SYMBOL, ARG_CONTRACT_WHITELIST, ARG_HOLDER_MODE, ARG_IDENTIFIER_MODE,
        ARG_JSON_SCHEMA, ARG_METADATA_MUTABILITY, ARG_MINTING_MODE, ARG_NFT_KIND,
        ARG_NFT_METADATA_KIND, ARG_OWNERSHIP_MODE, ARG_RECEIPT_NAME, ARG_TOTAL_TOKEN_SUPPLY,
        ARG_WHITELIST_MODE, BURNT_TOKENS, BURN_MODE, COLLECTION_NAME, COLLECTION_SYMBOL,
        CONTRACT_NAME, CONTRACT_VERSION, CONTRACT_WHITELIST, ENTRY_POINT_INIT, HASH_BY_INDEX,
        HASH_KEY_NAME, HOLDER_MODE, IDENTIFIER_MODE, INDEX_BY_HASH, INSTALLER, JSON_SCHEMA,
        METADATA_CEP78, METADATA_CUSTOM_VALIDATED, METADATA_MUTABILITY, METADATA_NFT721,
        METADATA_RAW, MIGRATION_FLAG, MINTING_MODE, NFT_KIND, NFT_METADATA_KIND,
        NUMBER_OF_MINTED_TOKENS, OPERATOR, OWNED_TOKENS, OWNERSHIP_MODE, PAGE_LIMIT, PAGE_TABLE,
        RECEIPT_NAME, TOKEN_COUNTS, TOKEN_ISSUERS, TOKEN_OWNERS, TOTAL_TOKEN_SUPPLY,
        WHITELIST_MODE,
    },
    error::NFTCoreError,
    modalities::{
        BurnMode, MetadataMutability, MintingMode, NFTHolderMode, NFTIdentifierMode, NFTKind,
        NFTMetadataKind, OwnershipMode, WhitelistMode,
    },
};

#[no_mangle]
pub extern "C" fn init() {
    // We only allow the init() entrypoint to be called once.
    // If COLLECTION_NAME uref already exists we revert since this implies that
    // the init() entrypoint has already been called.
    if utils::named_uref_exists(COLLECTION_NAME) {
        runtime::revert(NFTCoreError::ContractAlreadyInitialized);
    }

    // Only the installing account may call this method. All other callers are erroneous.
    let installing_account = utils::get_account_hash(
        INSTALLER,
        NFTCoreError::MissingInstaller,
        NFTCoreError::InvalidInstaller,
    );

    // We revert if caller is not the managing installing account
    if installing_account != runtime::get_caller() {
        runtime::revert(NFTCoreError::InvalidAccount)
    }

    // Start collecting the runtime arguments.
    let collection_name: String = utils::get_named_arg_with_user_errors(
        ARG_COLLECTION_NAME,
        NFTCoreError::MissingCollectionName,
        NFTCoreError::InvalidCollectionName,
    )
    .unwrap_or_revert();

    let collection_symbol: String = utils::get_named_arg_with_user_errors(
        ARG_COLLECTION_SYMBOL,
        NFTCoreError::MissingCollectionSymbol,
        NFTCoreError::InvalidCollectionSymbol,
    )
    .unwrap_or_revert();

    let total_token_supply: u64 = utils::get_named_arg_with_user_errors(
        ARG_TOTAL_TOKEN_SUPPLY,
        NFTCoreError::MissingTotalTokenSupply,
        NFTCoreError::InvalidTotalTokenSupply,
    )
    .unwrap_or_revert();

    let allow_minting: bool = utils::get_named_arg_with_user_errors(
        ARG_ALLOW_MINTING,
        NFTCoreError::MissingMintingStatus,
        NFTCoreError::InvalidMintingStatus,
    )
    .unwrap_or_revert();

    let minting_mode: MintingMode = utils::get_named_arg_with_user_errors::<u8>(
        ARG_MINTING_MODE,
        NFTCoreError::MissingMintingMode,
        NFTCoreError::InvalidMintingMode,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let ownership_mode: OwnershipMode = utils::get_named_arg_with_user_errors::<u8>(
        ARG_OWNERSHIP_MODE,
        NFTCoreError::MissingOwnershipMode,
        NFTCoreError::InvalidOwnershipMode,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let nft_kind: NFTKind = utils::get_named_arg_with_user_errors::<u8>(
        ARG_NFT_KIND,
        NFTCoreError::MissingNftKind,
        NFTCoreError::InvalidNftKind,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let holder_mode: NFTHolderMode = utils::get_named_arg_with_user_errors::<u8>(
        ARG_HOLDER_MODE,
        NFTCoreError::MissingHolderMode,
        NFTCoreError::InvalidHolderMode,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let whitelist_mode: WhitelistMode = utils::get_named_arg_with_user_errors::<u8>(
        ARG_WHITELIST_MODE,
        NFTCoreError::MissingWhitelistMode,
        NFTCoreError::InvalidWhitelistMode,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let contract_whitelist = utils::get_named_arg_with_user_errors::<Vec<ContractHash>>(
        ARG_CONTRACT_WHITELIST,
        NFTCoreError::MissingContractWhiteList,
        NFTCoreError::InvalidContractWhitelist,
    )
    .unwrap_or_revert();

    if WhitelistMode::Locked == whitelist_mode
        && contract_whitelist.is_empty()
        && NFTHolderMode::Accounts != holder_mode
    {
        runtime::revert(NFTCoreError::EmptyContractWhitelist)
    }

    let json_schema: String = utils::get_named_arg_with_user_errors(
        ARG_JSON_SCHEMA,
        NFTCoreError::MissingJsonSchema,
        NFTCoreError::InvalidJsonSchema,
    )
    .unwrap_or_revert();

    let receipt_name: String = utils::get_named_arg_with_user_errors(
        ARG_RECEIPT_NAME,
        NFTCoreError::MissingReceiptName,
        NFTCoreError::InvalidReceiptName,
    )
    .unwrap_or_revert();

    let nft_metadata_kind: NFTMetadataKind = utils::get_named_arg_with_user_errors::<u8>(
        ARG_NFT_METADATA_KIND,
        NFTCoreError::MissingNFTMetadataKind,
        NFTCoreError::InvalidNFTMetadataKind,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let identifier_mode: NFTIdentifierMode = utils::get_named_arg_with_user_errors::<u8>(
        ARG_IDENTIFIER_MODE,
        NFTCoreError::MissingIdentifierMode,
        NFTCoreError::InvalidIdentifierMode,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let metadata_mutability: MetadataMutability = utils::get_named_arg_with_user_errors::<u8>(
        ARG_METADATA_MUTABILITY,
        NFTCoreError::MissingMetadataMutability,
        NFTCoreError::InvalidMetadataMutability,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    let burn_mode: BurnMode = utils::get_named_arg_with_user_errors::<u8>(
        ARG_BURN_MODE,
        NFTCoreError::MissingBurnMode,
        NFTCoreError::InvalidBurnMode,
    )
    .unwrap_or_revert()
    .try_into()
    .unwrap_or_revert();

    // Put all created URefs into the contract's context (necessary to retain access rights,
    // for future use).
    //
    // Initialize contract with URefs for all invariant values, which can never be changed.
    runtime::put_key(COLLECTION_NAME, storage::new_uref(collection_name).into());
    runtime::put_key(
        COLLECTION_SYMBOL,
        storage::new_uref(collection_symbol).into(),
    );
    runtime::put_key(
        TOTAL_TOKEN_SUPPLY,
        storage::new_uref(total_token_supply).into(),
    );
    runtime::put_key(
        OWNERSHIP_MODE,
        storage::new_uref(ownership_mode as u8).into(),
    );
    runtime::put_key(NFT_KIND, storage::new_uref(nft_kind as u8).into());
    runtime::put_key(JSON_SCHEMA, storage::new_uref(json_schema).into());
    runtime::put_key(MINTING_MODE, storage::new_uref(minting_mode as u8).into());
    runtime::put_key(HOLDER_MODE, storage::new_uref(holder_mode as u8).into());
    runtime::put_key(
        WHITELIST_MODE,
        storage::new_uref(whitelist_mode as u8).into(),
    );
    runtime::put_key(
        CONTRACT_WHITELIST,
        storage::new_uref(contract_whitelist).into(),
    );
    runtime::put_key(RECEIPT_NAME, storage::new_uref(receipt_name).into());
    runtime::put_key(
        NFT_METADATA_KIND,
        storage::new_uref(nft_metadata_kind as u8).into(),
    );
    runtime::put_key(
        IDENTIFIER_MODE,
        storage::new_uref(identifier_mode as u8).into(),
    );
    runtime::put_key(
        METADATA_MUTABILITY,
        storage::new_uref(metadata_mutability as u8).into(),
    );
    runtime::put_key(BURN_MODE, storage::new_uref(burn_mode as u8).into());

    // Initialize contract with variables which must be present but maybe set to
    // different values after initialization.
    runtime::put_key(ALLOW_MINTING, storage::new_uref(allow_minting).into());
    // This is an internal variable that the installing account cannot change
    // but is incremented by the contract itself.
    runtime::put_key(NUMBER_OF_MINTED_TOKENS, storage::new_uref(0u64).into());

    // Create the data dictionaries to store essential values, topically.
    storage::new_dictionary(TOKEN_OWNERS)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(TOKEN_ISSUERS)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(OWNED_TOKENS)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(OPERATOR).unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(BURNT_TOKENS)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(TOKEN_COUNTS)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(METADATA_CUSTOM_VALIDATED)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(METADATA_CEP78)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(METADATA_NFT721)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(METADATA_RAW)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(HASH_BY_INDEX)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(INDEX_BY_HASH)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    storage::new_dictionary(PAGE_TABLE)
        .unwrap_or_revert_with(NFTCoreError::FailedToCreateDictionary);
    let page_table_width = utils::max_number_of_pages(total_token_supply);
    runtime::put_key(PAGE_LIMIT, storage::new_uref(page_table_width).into());
    runtime::put_key(MIGRATION_FLAG, storage::new_uref(true).into());
}

fn generate_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    // This entrypoint initializes the contract and is required to be called during the session
    // where the contract is installed; immediately after the contract has been installed but
    // before exiting session. All parameters are required.
    // This entrypoint is intended to be called exactly once and will error if called more than
    // once.
    let init_contract = EntryPoint::new(
        ENTRY_POINT_INIT,
        vec![
            Parameter::new(ARG_COLLECTION_NAME, CLType::String),
            Parameter::new(ARG_COLLECTION_SYMBOL, CLType::String),
            Parameter::new(ARG_TOTAL_TOKEN_SUPPLY, CLType::U64),
            Parameter::new(ARG_ALLOW_MINTING, CLType::Bool),
            Parameter::new(ARG_MINTING_MODE, CLType::U8),
            Parameter::new(ARG_OWNERSHIP_MODE, CLType::U8),
            Parameter::new(ARG_NFT_KIND, CLType::U8),
            Parameter::new(ARG_HOLDER_MODE, CLType::U8),
            Parameter::new(ARG_WHITELIST_MODE, CLType::U8),
            Parameter::new(
                ARG_CONTRACT_WHITELIST,
                CLType::List(Box::new(CLType::ByteArray(32u32))),
            ),
            Parameter::new(ARG_JSON_SCHEMA, CLType::String),
            Parameter::new(ARG_RECEIPT_NAME, CLType::String),
            Parameter::new(ARG_IDENTIFIER_MODE, CLType::U8),
            Parameter::new(ARG_BURN_MODE, CLType::U8),
            Parameter::new(ARG_NFT_METADATA_KIND, CLType::U8),
            Parameter::new(ARG_METADATA_MUTABILITY, CLType::U8),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    entry_points.add_entry_point(init_contract);
    entry_points
}

fn install_nft_contract() -> (ContractHash, ContractVersion) {
    let entry_points = generate_entry_points();

    let named_keys = {
        let mut named_keys = NamedKeys::new();
        named_keys.insert(INSTALLER.to_string(), runtime::get_caller().into());

        named_keys
    };

    storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    )
}

fn install_contract() {
    // Represents the name of the NFT collection
    // This value cannot be changed after installation.
    let collection_name: String = utils::get_named_arg_with_user_errors(
        ARG_COLLECTION_NAME,
        NFTCoreError::MissingCollectionName,
        NFTCoreError::InvalidCollectionName,
    )
    .unwrap_or_revert();

    // TODO: figure out examples of collection_symbol
    // The symbol for the NFT collection.
    // This value cannot be changed after installation.
    let collection_symbol: String = utils::get_named_arg_with_user_errors(
        ARG_COLLECTION_SYMBOL,
        NFTCoreError::MissingCollectionSymbol,
        NFTCoreError::InvalidCollectionSymbol,
    )
    .unwrap_or_revert();

    // This represents the total number of NFTs that will
    // be minted by a specific instance of a contract.
    // This value cannot be changed after installation.
    let total_token_supply: u64 = utils::get_named_arg_with_user_errors(
        ARG_TOTAL_TOKEN_SUPPLY,
        NFTCoreError::MissingTotalTokenSupply,
        NFTCoreError::InvalidTotalTokenSupply,
    )
    .unwrap_or_revert();

    if total_token_supply == 0 {
        runtime::revert(NFTCoreError::CannotInstallWithZeroSupply)
    }

    let allow_minting: bool = utils::get_optional_named_arg_with_user_errors(
        ARG_ALLOW_MINTING,
        NFTCoreError::InvalidMintingStatus,
    )
    .unwrap_or(true);

    // Represents the modes in which NFTs can be minted, i.e whether a singular known
    // entity v. users interacting with the contract. Refer to the `MintingMode`
    // enum in the `src/modalities.rs` file for details.
    // This value cannot be changed after installation.
    let minting_mode: u8 = utils::get_optional_named_arg_with_user_errors(
        ARG_MINTING_MODE,
        NFTCoreError::InvalidMintingMode,
    )
    .unwrap_or(0);

    // Represents the ownership model of the NFTs that will be minted
    // over the lifetime of the contract. Refer to the enum `OwnershipMode`
    // in the `src/modalities.rs` file for details.
    // This value cannot be changed after installation.
    let ownership_mode: u8 = utils::get_named_arg_with_user_errors(
        ARG_OWNERSHIP_MODE,
        NFTCoreError::MissingOwnershipMode,
        NFTCoreError::InvalidOwnershipMode,
    )
    .unwrap_or_revert();

    // Represents the type of NFT (i.e something physical/digital)
    // which will be minted over the lifetime of the contract.
    // Refer to the enum `NFTKind`
    // in the `src/modalities.rs` file for details.
    // This value cannot be changed after installation.
    let nft_kind: u8 = utils::get_named_arg_with_user_errors(
        ARG_NFT_KIND,
        NFTCoreError::MissingNftKind,
        NFTCoreError::InvalidNftKind,
    )
    .unwrap_or_revert();

    // Represents whether Accounts or Contracts, or both can hold NFTs for
    // a given contract instance. Refer to the enum `NFTHolderMode`
    // in the `src/modalities.rs` file for details.
    // This value cannot be changed after installation
    let holder_mode: u8 = utils::get_optional_named_arg_with_user_errors(
        ARG_HOLDER_MODE,
        NFTCoreError::InvalidHolderMode,
    )
    .unwrap_or(2u8);

    // Represents whether a given contract whitelist can be modified
    // for a given NFT contract instance. If not provided as an argument
    // it will default to unlocked.
    // This value cannot be changed after installation
    let whitelist_lock: u8 = utils::get_optional_named_arg_with_user_errors(
        ARG_WHITELIST_MODE,
        NFTCoreError::InvalidWhitelistMode,
    )
    .unwrap_or(0u8);

    // A whitelist of contract hashes specifying which contracts can mint
    // NFTs in the contract holder mode with restricted minting.
    // This value can only be modified if the whitelist lock is
    // set to be unlocked.
    let contract_white_list: Vec<ContractHash> = utils::get_optional_named_arg_with_user_errors(
        ARG_CONTRACT_WHITELIST,
        NFTCoreError::InvalidContractWhitelist,
    )
    .unwrap_or_default();

    // Represents the schema for the metadata for a given NFT contract instance.
    // Refer to the `NFTMetadataKind` enum in src/utils for details.
    // This value cannot be changed after installation.
    let nft_metadata_kind: u8 = utils::get_named_arg_with_user_errors(
        ARG_NFT_METADATA_KIND,
        NFTCoreError::MissingNFTMetadataKind,
        NFTCoreError::InvalidNFTMetadataKind,
    )
    .unwrap_or_revert();

    // The JSON schema representation of the NFT which will be minted.
    // This value cannot be changed after installation.
    let json_schema: String = utils::get_named_arg_with_user_errors(
        ARG_JSON_SCHEMA,
        NFTCoreError::MissingJsonSchema,
        NFTCoreError::InvalidJsonSchema,
    )
    .unwrap_or_revert();

    // Represents whether NFTs minted by a given contract will be identified
    // by an ordinal u64 index or a base16 encoded SHA256 hash of an NFTs metadata.
    // This value cannot be changed after installation. Refer to `NFTIdentifierMode` in
    // `src/modalities.rs` for further details.
    let identifier_mode: u8 = utils::get_named_arg_with_user_errors(
        ARG_IDENTIFIER_MODE,
        NFTCoreError::MissingIdentifierMode,
        NFTCoreError::InvalidIdentifierMode,
    )
    .unwrap_or_revert();

    // Represents whether the metadata related to NFTs can be updated.
    // This value cannot be changed after installation. Refer to `MetadataMutability` in
    // `src/modalities.rs` for further details.
    let metadata_mutability: u8 = utils::get_named_arg_with_user_errors(
        ARG_METADATA_MUTABILITY,
        NFTCoreError::MissingMetadataMutability,
        NFTCoreError::InvalidMetadataMutability,
    )
    .unwrap_or_revert();

    if identifier_mode == 1 && metadata_mutability == 1 {
        runtime::revert(NFTCoreError::InvalidMetadataMutability)
    }

    // Represents whether the minted tokens can be burnt.
    // This value cannot be changed post installation. Refer to `BurnMode` in
    // `src/modalities.rs` for further details.
    let burn_mode: u8 = utils::get_optional_named_arg_with_user_errors(
        ARG_BURN_MODE,
        NFTCoreError::InvalidBurnMode,
    )
    .unwrap_or(0u8);

    let (contract_hash, contract_version) = install_nft_contract();

    // Store contract_hash and contract_version under the keys CONTRACT_NAME and CONTRACT_VERSION
    runtime::put_key(CONTRACT_NAME, contract_hash.into());
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());

    let package_hash: ContractPackageHash = runtime::get_key(HASH_KEY_NAME)
        .unwrap_or_revert()
        .into_hash()
        .map(ContractPackageHash::new)
        .unwrap();

    // A sentinel string value which represents the entry for the addition
    // of a read only reference to the NFTs owned by the calling `Account` or `Contract`
    // This allows for users to look up a set of named keys and correctly identify
    // the contract package from which the NFTs were obtained.
    let receipt_name = format!("cep78-{}-{}", collection_name, package_hash);

    // Call contract to initialize it
    runtime::call_contract::<()>(
        contract_hash,
        ENTRY_POINT_INIT,
        runtime_args! {
             ARG_COLLECTION_NAME => collection_name,
             ARG_COLLECTION_SYMBOL => collection_symbol,
             ARG_TOTAL_TOKEN_SUPPLY => total_token_supply,
             ARG_ALLOW_MINTING => allow_minting,
             ARG_OWNERSHIP_MODE => ownership_mode,
             ARG_NFT_KIND => nft_kind,
             ARG_MINTING_MODE => minting_mode,
             ARG_HOLDER_MODE => holder_mode,
             ARG_WHITELIST_MODE => whitelist_lock,
             ARG_CONTRACT_WHITELIST => contract_white_list,
             ARG_JSON_SCHEMA => json_schema,
             ARG_RECEIPT_NAME => receipt_name,
             ARG_NFT_METADATA_KIND => nft_metadata_kind,
             ARG_IDENTIFIER_MODE => identifier_mode,
             ARG_METADATA_MUTABILITY => metadata_mutability,
             ARG_BURN_MODE => burn_mode
        },
    );
}

#[no_mangle]
pub extern "C" fn call() {
    install_contract()
}
