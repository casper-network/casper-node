//! Types used to allow creation of Wasm contracts and tests for use on the Casper Platform.

#![cfg_attr(
    not(any(
        feature = "json-schema",
        feature = "datasize",
        feature = "std",
        feature = "testing",
        test,
    )),
    no_std
)]
#![doc(html_root_url = "https://docs.rs/casper-types/4.0.1")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon_48.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs)]

#[cfg_attr(not(test), macro_use)]
extern crate alloc;

mod access_rights;
pub mod account;
pub mod api_error;
mod block_time;
pub mod bytesrepr;
pub mod checksummed_hex;
mod cl_type;
mod cl_value;
mod contract_wasm;
pub mod contracts;
pub mod crypto;
mod deploy_info;
mod era_id;
mod execution_result;
#[cfg(any(feature = "std", test))]
pub mod file_utils;
mod gas;
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens;
mod json_pretty_printer;
mod key;
mod motes;
mod named_key;
mod phase;
mod protocol_version;
pub mod runtime_args;
mod semver;
mod stored_value;
pub mod system;
mod tagged;
#[cfg(any(feature = "testing", test))]
pub mod testing;
mod timestamp;
mod transfer;
mod transfer_result;
mod uint;
mod uref;

pub use access_rights::{
    AccessRights, ContextAccessRights, GrantedAccess, ACCESS_RIGHTS_SERIALIZED_LENGTH,
};
#[doc(inline)]
pub use api_error::ApiError;
pub use block_time::{BlockTime, BLOCKTIME_SERIALIZED_LENGTH};
pub use cl_type::{named_key_type, CLType, CLTyped};
pub use cl_value::{CLTypeMismatch, CLValue, CLValueError};
pub use contract_wasm::{ContractWasm, ContractWasmHash};
#[doc(inline)]
pub use contracts::{
    Contract, ContractHash, ContractPackage, ContractPackageHash, ContractVersion,
    ContractVersionKey, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Group,
    Parameter,
};
pub use crypto::*;
pub use deploy_info::DeployInfo;
pub use execution_result::{
    ExecutionEffect, ExecutionResult, OpKind, Operation, Transform, TransformEntry,
};
pub use gas::Gas;
pub use json_pretty_printer::json_pretty_print;
#[doc(inline)]
pub use key::{
    DictionaryAddr, FromStrError as KeyFromStrError, HashAddr, Key, KeyTag, BLAKE2B_DIGEST_LENGTH,
    DICTIONARY_ITEM_KEY_MAX_LENGTH, KEY_DICTIONARY_LENGTH, KEY_HASH_LENGTH,
};
pub use motes::Motes;
pub use named_key::NamedKey;
pub use phase::{Phase, PHASE_SERIALIZED_LENGTH};
pub use protocol_version::{ProtocolVersion, VersionCheckResult};
#[doc(inline)]
pub use runtime_args::{NamedArg, RuntimeArgs};
pub use semver::{ParseSemVerError, SemVer, SEM_VER_SERIALIZED_LENGTH};
pub use stored_value::{StoredValue, TypeMismatch as StoredValueTypeMismatch};
pub use tagged::Tagged;
#[cfg(any(feature = "std", test))]
pub use timestamp::serde_option_time_diff;
pub use timestamp::{TimeDiff, Timestamp};
pub use transfer::{
    DeployHash, FromStrError as TransferFromStrError, Transfer, TransferAddr, DEPLOY_HASH_LENGTH,
    TRANSFER_ADDR_LENGTH,
};
pub use transfer_result::{TransferResult, TransferredTo};
pub use uref::{
    FromStrError as URefFromStrError, URef, URefAddr, UREF_ADDR_LENGTH, UREF_SERIALIZED_LENGTH,
};

pub use crate::{
    era_id::EraId,
    uint::{UIntParseError, U128, U256, U512},
};
