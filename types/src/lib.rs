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
#![doc(html_root_url = "https://docs.rs/casper-types/5.0.0")]
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
pub mod global_state;
mod json_pretty_printer;
mod key;
mod motes;
mod package;
mod peers_map;
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

#[cfg(feature = "std")]
use libc::{c_long, sysconf, _SC_PAGESIZE};
#[cfg(feature = "std")]
use once_cell::sync::Lazy;

pub use crate::uint::{UIntParseError, U128, U256, U512};

pub use access_rights::{
    AccessRights, ContextAccessRights, GrantedAccess, ACCESS_RIGHTS_SERIALIZED_LENGTH,
};
pub use account::Account;
#[doc(inline)]
pub use addressable_entity::{
    AddressableEntity, AddressableEntityHash, EntityAddr, EntityKind, EntryPoint, EntryPointAccess,
    EntryPointAddr, EntryPointPayment, EntryPointType, EntryPointValue, EntryPoints, Parameter,
    Parameters, DEFAULT_ENTRY_POINT_NAME,
};
#[doc(inline)]
pub use api_error::ApiError;
pub use auction_state::{AuctionState, JsonEraValidators, JsonValidatorWeights};
#[cfg(all(feature = "std", feature = "json-schema"))]
pub use block::JsonBlockWithSignatures;
pub use block::{
    AvailableBlockRange, Block, BlockBody, BlockBodyV1, BlockBodyV2, BlockGlobalAddr,
    BlockGlobalAddrTag, BlockHash, BlockHashAndHeight, BlockHeader, BlockHeaderV1, BlockHeaderV2,
    BlockIdentifier, BlockSignatures, BlockSignaturesMergeError, BlockSignaturesV1,
    BlockSignaturesV2, BlockSyncStatus, BlockSynchronizerStatus, BlockV1, BlockV2,
    BlockValidationError, ChainNameDigest, EraEnd, EraEndV1, EraEndV2, EraReport,
    FinalitySignature, FinalitySignatureId, FinalitySignatureV1, FinalitySignatureV2,
    RewardedSignatures, Rewards, SignedBlock, SignedBlockHeader, SignedBlockHeaderValidationError,
    SingleBlockRewardedSignatures,
};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
pub use block::{TestBlockBuilder, TestBlockV1Builder};
pub use block_time::{BlockTime, HoldsEpoch, BLOCKTIME_SERIALIZED_LENGTH};
pub use byte_code::{ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind};
pub use global_state::Pointer;

#[cfg(any(feature = "std", test))]
pub use chainspec::{
    AccountConfig, AccountsConfig, ActivationPoint, AdministratorAccount, AuctionCosts,
    BrTableCost, Chainspec, ChainspecRawBytes, ChainspecRegistry, ConsensusProtocolName,
    ControlFlowCosts, CoreConfig, DelegatorConfig, DeployConfig, EntityCosts, FeeHandling,
    GenesisAccount, GenesisConfig, GenesisConfigBuilder, GenesisValidator, GlobalStateUpdate,
    GlobalStateUpdateConfig, GlobalStateUpdateError, HandlePaymentCosts, HighwayConfig,
    HoldBalanceHandling, HostFunction, HostFunctionCost, HostFunctionCosts, LegacyRequiredFinality,
    MessageLimits, MintCosts, NetworkConfig, NextUpgrade, OpcodeCosts, PricingHandling,
    ProtocolConfig, ProtocolUpgradeConfig, RefundHandling, StandardPaymentCosts, StorageCosts,
    SystemConfig, TransactionConfig, TransactionV1Config, VacancyConfig, ValidatorConfig,
    WasmConfig, DEFAULT_GAS_HOLD_INTERVAL, DEFAULT_HOST_FUNCTION_NEW_DICTIONARY,
    DEFAULT_REFUND_HANDLING,
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
    DEFAULT_FEE_HANDLING, DEFAULT_GAS_HOLD_BALANCE_HANDLING, DEFAULT_GLOBAL_COST,
    DEFAULT_GROW_MEMORY_COST, DEFAULT_INSTALL_UPGRADE_GAS_LIMIT, DEFAULT_INTEGER_COMPARISON_COST,
    DEFAULT_LOAD_COST, DEFAULT_LOCAL_COST, DEFAULT_MAX_PAYMENT_MOTES, DEFAULT_MAX_STACK_HEIGHT,
    DEFAULT_MIN_TRANSFER_MOTES, DEFAULT_MUL_COST, DEFAULT_NEW_DICTIONARY_COST, DEFAULT_NOP_COST,
    DEFAULT_STANDARD_TRANSACTION_GAS_LIMIT, DEFAULT_STORE_COST, DEFAULT_TRANSFER_COST,
    DEFAULT_UNREACHABLE_COST, DEFAULT_WASM_MAX_MEMORY,
};
pub use cl_type::{named_key_type, CLType, CLTyped};
pub use cl_value::{
    handle_stored_dictionary_value, CLTypeMismatch, CLValue, CLValueError, ChecksumRegistry,
    DictionaryValue as CLValueDictionary, SystemEntityRegistry,
};
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
    DictionaryAddr, FromStrError as KeyFromStrError, HashAddr, Key, KeyTag, PackageAddr,
    BLAKE2B_DIGEST_LENGTH, DICTIONARY_ITEM_KEY_MAX_LENGTH, KEY_DICTIONARY_LENGTH, KEY_HASH_LENGTH,
};
pub use motes::Motes;
#[doc(inline)]
pub use package::{
    EntityVersion, EntityVersionKey, EntityVersions, Group, Groups, Package, PackageHash,
    PackageStatus, ENTITY_INITIAL_VERSION,
};
pub use peers_map::{PeerEntry, Peers};
pub use phase::{Phase, PHASE_SERIALIZED_LENGTH};
pub use protocol_version::{ProtocolVersion, VersionCheckResult};
pub use semver::{ParseSemVerError, SemVer, SEM_VER_SERIALIZED_LENGTH};
pub use stored_value::{
    GlobalStateIdentifier, StoredValue, TypeMismatch as StoredValueTypeMismatch,
};
pub use tagged::Tagged;
#[cfg(any(feature = "std", test))]
pub use timestamp::serde_option_time_diff;
pub use timestamp::{TimeDiff, Timestamp};
#[cfg(any(feature = "std", test))]
pub use transaction::GasLimited;
pub use transaction::{
    AddressableEntityIdentifier, Approval, ApprovalsHash, Categorized as CategorizedTransaction,
    Deploy, DeployDecodeFromJsonError, DeployError, DeployExcessiveSizeError, DeployHash,
    DeployHeader, DeployId, ExecutableDeployItem, ExecutableDeployItemIdentifier, ExecutionInfo,
    InitiatorAddr, InvalidDeploy, InvalidTransaction, InvalidTransactionV1, NamedArg,
    PackageIdentifier, PricingMode, RuntimeArgs, Transaction, TransactionCategory,
    TransactionEntryPoint, TransactionHash, TransactionHeader, TransactionId,
    TransactionInvocationTarget, TransactionRuntime, TransactionScheduling, TransactionSessionKind,
    TransactionTarget, TransactionV1, TransactionV1Body, TransactionV1DecodeFromJsonError,
    TransactionV1Error, TransactionV1ExcessiveSizeError, TransactionV1Hash, TransactionV1Header,
    TransferTarget,
};
#[cfg(any(feature = "std", test))]
pub use transaction::{
    DeployBuilder, DeployBuilderError, TransactionV1Builder, TransactionV1BuilderError,
};
pub use transfer::{
    Transfer, TransferAddr, TransferFromStrError, TransferV1, TransferV2, TRANSFER_ADDR_LENGTH,
};
pub use transfer_result::{TransferResult, TransferredTo};
pub use uref::{
    FromStrError as URefFromStrError, URef, URefAddr, UREF_ADDR_LENGTH, UREF_SERIALIZED_LENGTH,
};
pub use validator_change::ValidatorChange;

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
