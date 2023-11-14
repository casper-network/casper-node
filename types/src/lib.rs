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
#![doc(html_root_url = "https://docs.rs/casper-types/3.0.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon_48.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon.png"
)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg_attr(not(test), macro_use)]
extern crate alloc;
extern crate core;

mod access_rights;
pub mod account;
pub mod addressable_entity;
pub mod api_error;
mod auction_state;
mod block;
mod block_time;
mod byte_code;
pub mod bytesrepr;
#[cfg(any(feature = "std", test))]
mod chainspec;
pub mod checksummed_hex;
mod cl_type;
mod cl_value;
pub mod contract_messages;
mod contract_wasm;
pub mod contracts;
pub mod crypto;
mod deploy_info;
mod digest;
mod display_iter;
mod era_id;
pub mod execution;
#[cfg(any(feature = "std", test))]
pub mod file_utils;
mod gas;
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens;
mod json_pretty_printer;
mod key;
mod motes;
pub mod package;
mod phase;
mod protocol_version;
mod semver;
pub(crate) mod serde_helpers;
mod stored_value;
pub mod system;
mod tagged;
#[cfg(any(feature = "testing", test))]
pub mod testing;
mod timestamp;
mod transaction;
mod transfer;
mod transfer_result;
mod uint;
mod uref;
mod validator_change;

use bytesrepr::{Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(feature = "std")]
use libc::{c_long, sysconf, _SC_PAGESIZE};
#[cfg(feature = "std")]
use once_cell::sync::Lazy;
use serde::Serialize;

pub use crate::uint::{UIntParseError, U128, U256, U512};
pub use access_rights::{
    AccessRights, ContextAccessRights, GrantedAccess, ACCESS_RIGHTS_SERIALIZED_LENGTH,
};
#[doc(inline)]
pub use addressable_entity::{
    AddressableEntity, AddressableEntityHash, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Parameter,
};
#[doc(inline)]
pub use api_error::ApiError;
#[cfg(all(feature = "std", feature = "json-schema"))]
pub use block::JsonBlockWithSignatures;
pub use block::{
    AvailableBlockRange, Block, BlockBody, BlockBodyV1, BlockBodyV2, BlockHash, BlockHeader,
    BlockHeaderV1, BlockHeaderV2, BlockSignatures, BlockSignaturesMergeError, BlockV1, BlockV2,
    BlockValidationError, EraEnd, EraEndV1, EraEndV2, EraReport, FinalitySignature,
    FinalitySignatureId, RewardedSignatures, Rewards, SignedBlock, SignedBlockHeader,
    SignedBlockHeaderValidationError, SingleBlockRewardedSignatures,
};
#[cfg(any(feature = "testing", test))]
pub use block::{TestBlockBuilder, TestBlockV1Builder};

pub use auction_state::{AuctionState, JsonEraValidators, JsonValidatorWeights};
pub use block_time::{BlockTime, BLOCKTIME_SERIALIZED_LENGTH};
pub use byte_code::{ByteCode, ByteCodeHash, ByteCodeKind};
#[cfg(any(feature = "std", test))]
pub use chainspec::{
    AccountConfig, AccountsConfig, ActivationPoint, AdministratorAccount, AuctionCosts,
    BrTableCost, Chainspec, ChainspecRawBytes, ChainspecRegistry, ConsensusProtocolName,
    ControlFlowCosts, CoreConfig, DelegatorConfig, DeployConfig, FeeHandling, GenesisAccount,
    GenesisValidator, GlobalStateUpdate, GlobalStateUpdateConfig, GlobalStateUpdateError,
    HandlePaymentCosts, HighwayConfig, HostFunction, HostFunctionCost, HostFunctionCosts,
    LegacyRequiredFinality, MessageLimits, MintCosts, NetworkConfig, OpcodeCosts, ProtocolConfig,
    RefundHandling, StandardPaymentCosts, StorageCosts, SystemConfig, TransactionConfig,
    TransactionV1Config, UpgradeConfig, ValidatorConfig, WasmConfig,
    DEFAULT_HOST_FUNCTION_NEW_DICTIONARY,
};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
pub use chainspec::{
    DEFAULT_ADD_BID_COST, DEFAULT_ADD_COST, DEFAULT_BIT_COST, DEFAULT_CONST_COST,
    DEFAULT_CONTROL_FLOW_BLOCK_OPCODE, DEFAULT_CONTROL_FLOW_BR_IF_OPCODE,
    DEFAULT_CONTROL_FLOW_BR_OPCODE, DEFAULT_CONTROL_FLOW_BR_TABLE_MULTIPLIER,
    DEFAULT_CONTROL_FLOW_BR_TABLE_OPCODE, DEFAULT_CONTROL_FLOW_CALL_INDIRECT_OPCODE,
    DEFAULT_CONTROL_FLOW_CALL_OPCODE, DEFAULT_CONTROL_FLOW_DROP_OPCODE,
    DEFAULT_CONTROL_FLOW_ELSE_OPCODE, DEFAULT_CONTROL_FLOW_END_OPCODE,
    DEFAULT_CONTROL_FLOW_IF_OPCODE, DEFAULT_CONTROL_FLOW_LOOP_OPCODE,
    DEFAULT_CONTROL_FLOW_RETURN_OPCODE, DEFAULT_CONTROL_FLOW_SELECT_OPCODE,
    DEFAULT_CONVERSION_COST, DEFAULT_CURRENT_MEMORY_COST, DEFAULT_DELEGATE_COST, DEFAULT_DIV_COST,
    DEFAULT_GLOBAL_COST, DEFAULT_GROW_MEMORY_COST, DEFAULT_INTEGER_COMPARISON_COST,
    DEFAULT_LOAD_COST, DEFAULT_LOCAL_COST, DEFAULT_MAX_PAYMENT_MOTES, DEFAULT_MAX_STACK_HEIGHT,
    DEFAULT_MIN_TRANSFER_MOTES, DEFAULT_MUL_COST, DEFAULT_NEW_DICTIONARY_COST, DEFAULT_NOP_COST,
    DEFAULT_STORE_COST, DEFAULT_TRANSFER_COST, DEFAULT_UNREACHABLE_COST,
    DEFAULT_WASMLESS_TRANSFER_COST, DEFAULT_WASM_MAX_MEMORY,
};
pub use cl_type::{named_key_type, CLType, CLTyped};
pub use cl_value::{CLTypeMismatch, CLValue, CLValueError};
pub use contract_wasm::ContractWasm;
#[doc(inline)]
pub use contracts::Contract;
pub use crypto::*;
pub use deploy_info::DeployInfo;
pub use digest::{
    ChunkWithProof, ChunkWithProofVerificationError, Digest, DigestError, IndexedMerkleProof,
    MerkleConstructionError, MerkleVerificationError,
};
pub use display_iter::DisplayIter;
pub use era_id::EraId;
pub use gas::Gas;
pub use json_pretty_printer::json_pretty_print;
#[doc(inline)]
pub use key::{
    ByteCodeAddr, DictionaryAddr, EntityAddr, FromStrError as KeyFromStrError, HashAddr, Key,
    KeyTag, PackageAddr, BLAKE2B_DIGEST_LENGTH, DICTIONARY_ITEM_KEY_MAX_LENGTH,
    KEY_DICTIONARY_LENGTH, KEY_HASH_LENGTH,
};
pub use motes::Motes;
#[doc(inline)]
pub use package::{
    EntityVersion, EntityVersionKey, EntityVersions, Group, Groups, Package, PackageHash,
};
pub use phase::{Phase, PHASE_SERIALIZED_LENGTH};
pub use protocol_version::{ProtocolVersion, VersionCheckResult};
pub use semver::{ParseSemVerError, SemVer, SEM_VER_SERIALIZED_LENGTH};
pub use stored_value::{StoredValue, TypeMismatch as StoredValueTypeMismatch};
pub use tagged::Tagged;
#[cfg(any(feature = "std", test))]
pub use timestamp::serde_option_time_diff;
pub use timestamp::{TimeDiff, Timestamp};
pub use transaction::{
    AddressableEntityIdentifier, Deploy, DeployApproval, DeployApprovalsHash, DeployConfigFailure,
    DeployDecodeFromJsonError, DeployError, DeployExcessiveSizeError, DeployFootprint, DeployHash,
    DeployHeader, DeployId, ExecutableDeployItem, ExecutableDeployItemIdentifier, InitiatorAddr,
    NamedArg, PackageIdentifier, PricingMode, RuntimeArgs, Transaction, TransactionApprovalsHash,
    TransactionEntryPoint, TransactionHash, TransactionHeader, TransactionId,
    TransactionInvocationTarget, TransactionRuntime, TransactionScheduling, TransactionSessionKind,
    TransactionTarget, TransactionV1, TransactionV1Approval, TransactionV1ApprovalsHash,
    TransactionV1Body, TransactionV1ConfigFailure, TransactionV1DecodeFromJsonError,
    TransactionV1Error, TransactionV1ExcessiveSizeError, TransactionV1Hash, TransactionV1Header,
    TransferTarget,
};
#[cfg(any(feature = "std", test))]
pub use transaction::{
    DeployBuilder, DeployBuilderError, TransactionV1Builder, TransactionV1BuilderError,
};
pub use transfer::{
    FromStrError as TransferFromStrError, Transfer, TransferAddr, TRANSFER_ADDR_LENGTH,
};
pub use transfer_result::{TransferResult, TransferredTo};
pub use uref::{
    FromStrError as URefFromStrError, URef, URefAddr, UREF_ADDR_LENGTH, UREF_SERIALIZED_LENGTH,
};
pub use validator_change::ValidatorChange;

use alloc::{string::String, vec::Vec};

/// OS page size.
#[cfg(feature = "std")]
pub static OS_PAGE_SIZE: Lazy<usize> = Lazy::new(|| {
    /// Sensible default for many if not all systems.
    const DEFAULT_PAGE_SIZE: usize = 4096;

    // https://www.gnu.org/software/libc/manual/html_node/Sysconf.html
    let value: c_long = unsafe { sysconf(_SC_PAGESIZE) };
    if value <= 0 {
        DEFAULT_PAGE_SIZE
    } else {
        value as usize
    }
});

const ERROR_TAG: u8 = 0;

/// TODO[RC]: Move the below to proper place.

/// TODO
#[derive(Debug)]
pub enum BinaryError {
    /// TODO
    Error(String),
}

const GET_TAG: u8 = 0;
const PUT_TRANSACTION_TAG: u8 = 1;
const SPECULATIVE_EXEC_TAG: u8 = 2;
const IN_MEM_REQUEST_TAG: u8 = 3;
const QUIT_EXEC_TAG: u8 = 4;

const BLOCK_HEADER_DB_TAG: u8 = 0;
const BLOCK_HEADER_V2_DB_TAG: u8 = 1;
const BLOCK_METADATA_DB_TAG: u8 = 2;
const DEPLOYS_DB_TAG: u8 = 3;
const TRANSACTIONS_DB_TAG: u8 = 4;
const DEPLOY_METADATA_DB_TAG: u8 = 5;
const EXECUTION_RESULTS_DB_TAG: u8 = 6;
const TRANSFER_DB_TAG: u8 = 7;
const STATE_STORE_DB_TAG: u8 = 8;
const BLOCKBODY_DB_TAG: u8 = 9;
const BLOCKBODY_V2_DB_TAG: u8 = 10;
const FINALIZED_APPROVALS_DB_TAG: u8 = 11;
const VERSIONED_FINALIZED_APPROVALS_DB_TAG: u8 = 12;
const APPROVALS_HASHES_DB_TAG: u8 = 13;
const VERSIONED_APPROVALS_HASHES_DB_TAG: u8 = 14;

#[derive(Debug)]
pub enum InMemRequest {
    /// Returns hash for a given height
    BlockHeight2Hash { height: u64 },
    /// Returns height&hash for the currently highest block
    HighestBlock,
    /// Returns true if `self.completed_blocks.highest_sequence()` contains the given hash
    CompletedBlockContains { block_hash: BlockHash },
    /// TODO
    TransactionHash2BlockHashAndHeight { transaction_hash: Digest },
}

const BLOCK_HEIGHT_2_HASH_TAG: u8 = 0;
const HIGHEST_BLOCK_TAG: u8 = 1;
const COMPLETED_BLOCK_CONTAINS_TAG: u8 = 2;
const TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG: u8 = 3;

impl ToBytes for InMemRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            InMemRequest::BlockHeight2Hash { height } => {
                ERROR_TAG.write_bytes(writer)?;
                height.write_bytes(writer)
            }
            InMemRequest::HighestBlock => HIGHEST_BLOCK_TAG.write_bytes(writer),
            InMemRequest::CompletedBlockContains { block_hash } => {
                COMPLETED_BLOCK_CONTAINS_TAG.write_bytes(writer)?;
                block_hash.write_bytes(writer)
            }
            InMemRequest::TransactionHash2BlockHashAndHeight { transaction_hash } => {
                TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG.write_bytes(writer)?;
                transaction_hash.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                InMemRequest::BlockHeight2Hash { height } => height.serialized_length(),
                InMemRequest::HighestBlock => 0,
                InMemRequest::CompletedBlockContains { block_hash } => {
                    block_hash.serialized_length()
                }
                InMemRequest::TransactionHash2BlockHashAndHeight { transaction_hash } => {
                    transaction_hash.serialized_length()
                }
            }
    }
}

impl FromBytes for InMemRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_HEIGHT_2_HASH_TAG => {
                let (height, remainder) = u64::from_bytes(remainder)?;
                Ok((InMemRequest::BlockHeight2Hash { height }, remainder))
            }
            HIGHEST_BLOCK_TAG => Ok((InMemRequest::HighestBlock, remainder)),
            COMPLETED_BLOCK_CONTAINS_TAG => {
                let (block_hash, remainder) = BlockHash::from_bytes(remainder)?;
                Ok((
                    InMemRequest::CompletedBlockContains { block_hash },
                    remainder,
                ))
            }
            TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG => {
                let (transaction_hash, remainder) = Digest::from_bytes(remainder)?;
                Ok((
                    InMemRequest::TransactionHash2BlockHashAndHeight { transaction_hash },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// TODO
#[derive(Debug)]
pub enum BinaryRequest {
    // TODO[RC] Add version tag, or rather follow the `BinaryRequestV1/V2` scheme.
    /// TODO
    Get {
        /// TODO
        db: DbId,
        /// TODO - bytesrepr serialized
        key: Vec<u8>,
    },
    InMemRequest(InMemRequest),
    /// TODO
    PutTransaction {
        /// TODO
        tbd: u32,
    },
    /// TODO
    SpeculativeExec {
        /// TODO
        tbd: u32,
    },
    /// TODO
    Quit,
}

/// TODO
/// First byte - error code, then raw data from DB
#[derive(Debug)]
pub struct BinaryResponse(pub Vec<u8>);

// TODO[RC]: Roundtrip tests for serialization
impl ToBytes for BinaryError {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BinaryError::Error(msg) => {
                ERROR_TAG.write_bytes(writer)?;
                msg.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BinaryError::Error(msg) => msg.serialized_length(),
            }
    }
}

impl FromBytes for BinaryError {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ERROR_TAG => {
                let (msg, remainder) = String::from_bytes(remainder)?;
                Ok((BinaryError::Error(msg), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl ToBytes for BinaryRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BinaryRequest::Get { db, key } => {
                GET_TAG.write_bytes(writer)?;
                db.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            BinaryRequest::PutTransaction { tbd } => {
                PUT_TRANSACTION_TAG.write_bytes(writer)?;
                tbd.write_bytes(writer)
            }
            BinaryRequest::SpeculativeExec { tbd } => {
                SPECULATIVE_EXEC_TAG.write_bytes(writer)?;
                tbd.write_bytes(writer)
            }
            BinaryRequest::Quit => QUIT_EXEC_TAG.write_bytes(writer),
            BinaryRequest::InMemRequest(req) => {
                IN_MEM_REQUEST_TAG.write_bytes(writer)?;
                req.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BinaryRequest::Get { db, key } => db.serialized_length() + key.serialized_length(),
                BinaryRequest::PutTransaction { tbd } => tbd.serialized_length(),
                BinaryRequest::SpeculativeExec { tbd } => tbd.serialized_length(),
                BinaryRequest::Quit => 0,
                BinaryRequest::InMemRequest(req) => req.serialized_length(),
            }
    }
}

impl FromBytes for BinaryRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            GET_TAG => {
                let (db, remainder) = DbId::from_bytes(remainder)?;
                let (key, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::Get {
                        db,
                        key: key.into(),
                    },
                    remainder,
                ))
            }
            PUT_TRANSACTION_TAG => {
                let (tbd, remainder) = u32::from_bytes(remainder)?;
                Ok((BinaryRequest::PutTransaction { tbd }, remainder))
            }
            SPECULATIVE_EXEC_TAG => {
                let (tbd, remainder) = u32::from_bytes(remainder)?;
                Ok((BinaryRequest::SpeculativeExec { tbd }, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Serialize)]
#[repr(u8)]
pub enum DbId {
    BlockHeader = 0,
    BlockHeaderV2 = 1,
    BlockMetadata = 2,
    Deploys = 3,
    Transactions = 4,
    DeployMetadata = 5,
    ExecutionResults = 6,
    Transfer = 7,
    StateStore = 8,
    BlockBody = 9,
    BlockBodyV2 = 10,
    FinalizedApprovals = 11,
    VersionedFinalizedApprovals = 12,
    ApprovalsHashes = 13,
    VersionedApprovalsHashes = 14,
}

impl std::fmt::Display for DbId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DbId::BlockHeader => write!(f, "BlockHeader"),
            DbId::BlockHeaderV2 => write!(f, "BlockHeaderV2"),
            DbId::BlockMetadata => write!(f, "BlockMetadata"),
            DbId::Deploys => write!(f, "Deploys"),
            DbId::Transactions => write!(f, "Transactions"),
            DbId::DeployMetadata => write!(f, "DeployMetadata"),
            DbId::ExecutionResults => write!(f, "ExecutionResults"),
            DbId::Transfer => write!(f, "Transfer"),
            DbId::StateStore => write!(f, "StateStore"),
            DbId::BlockBody => write!(f, "BlockBody"),
            DbId::BlockBodyV2 => write!(f, "BlockBodyV2"),
            DbId::FinalizedApprovals => write!(f, "FinalizedApprovals"),
            DbId::VersionedFinalizedApprovals => write!(f, "VersionedFinalizedApprovals"),
            DbId::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            DbId::VersionedApprovalsHashes => write!(f, "VersionedApprovalsHashes"),
        }
    }
}

impl ToBytes for DbId {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            // TODO[RC]: Do this less verbosely, plausibly by serializing as u8
            DbId::BlockHeader => BLOCK_HEADER_DB_TAG,
            DbId::BlockHeaderV2 => BLOCK_HEADER_V2_DB_TAG,
            DbId::BlockMetadata => BLOCK_METADATA_DB_TAG,
            DbId::Deploys => DEPLOYS_DB_TAG,
            DbId::Transactions => TRANSACTIONS_DB_TAG,
            DbId::DeployMetadata => DEPLOY_METADATA_DB_TAG,
            DbId::ExecutionResults => EXECUTION_RESULTS_DB_TAG,
            DbId::Transfer => TRANSFER_DB_TAG,
            DbId::StateStore => STATE_STORE_DB_TAG,
            DbId::BlockBody => BLOCKBODY_DB_TAG,
            DbId::BlockBodyV2 => BLOCKBODY_V2_DB_TAG,
            DbId::FinalizedApprovals => FINALIZED_APPROVALS_DB_TAG,
            DbId::VersionedFinalizedApprovals => VERSIONED_FINALIZED_APPROVALS_DB_TAG,
            DbId::ApprovalsHashes => APPROVALS_HASHES_DB_TAG,
            DbId::VersionedApprovalsHashes => VERSIONED_APPROVALS_HASHES_DB_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for DbId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        // TODO[RC]: Error handling
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let db_id = match tag {
            BLOCK_HEADER_DB_TAG => DbId::BlockHeader,
            BLOCK_HEADER_V2_DB_TAG => DbId::BlockHeaderV2,
            BLOCK_METADATA_DB_TAG => DbId::BlockMetadata,
            DEPLOYS_DB_TAG => DbId::Deploys,
            TRANSACTIONS_DB_TAG => DbId::Transactions,
            DEPLOY_METADATA_DB_TAG => DbId::DeployMetadata,
            EXECUTION_RESULTS_DB_TAG => DbId::ExecutionResults,
            TRANSFER_DB_TAG => DbId::Transfer,
            STATE_STORE_DB_TAG => DbId::StateStore,
            BLOCKBODY_DB_TAG => DbId::BlockBody,
            BLOCKBODY_V2_DB_TAG => DbId::BlockBodyV2,
            FINALIZED_APPROVALS_DB_TAG => DbId::FinalizedApprovals,
            VERSIONED_FINALIZED_APPROVALS_DB_TAG => DbId::VersionedFinalizedApprovals,
            APPROVALS_HASHES_DB_TAG => DbId::ApprovalsHashes,
            VERSIONED_APPROVALS_HASHES_DB_TAG => DbId::VersionedApprovalsHashes,
            _ => panic!("incorrect db"),
        };
        Ok((db_id, remainder))
    }
}
