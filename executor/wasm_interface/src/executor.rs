use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use borsh::BorshSerialize;
use bytes::Bytes;
use casper_storage::{
    global_state::{error::Error as GlobalStateError, GlobalStateReader},
    tracking_copy::TrackingCopyCache,
    AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash, contract_messages::Messages, execution::Effects, BlockHash, BlockTime,
    Digest, HashAddr, Key, TransactionHash,
};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use parking_lot::RwLock;
use thiserror::Error;

use crate::{
    CallError, FatalHostError, GasUsage, SandboxedExecutionRequest, SandboxedExecutionResult,
    WasmPreparationError,
};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

/// Request to execute a Wasm contract.
#[derive(Debug)]
pub struct ExecuteRequest {
    /// Initiator's address.
    pub initiator: AccountHash,
    /// Caller's address key.
    ///
    /// Either a `[`Key::Account`]` or a `[`Key::AddressableEntity`].
    pub caller_key: Key,
    /// Gas limit.
    pub gas_limit: u64,
    /// Target for execution.
    pub execution_kind: ExecutionKind,
    /// Input data.
    pub input: Bytes,
    /// Value transferred to the contract.
    pub transferred_value: u64,
    /// Transaction hash.
    pub transaction_hash: TransactionHash,
    /// Address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of
    /// execution requests.
    pub address_generator: Arc<RwLock<AddressGenerator>>,
    /// Chain name.
    ///
    /// This is very important ingredient for deriving contract hashes on the network.
    pub chain_name: Arc<str>,
    /// Block time represented as a unix timestamp.
    pub block_time: BlockTime,
    /// State root hash of the global state in which the transaction will be executed.
    pub state_hash: Digest,
    /// Parent block hash.
    pub parent_block_hash: BlockHash,
    /// Block height.
    pub block_height: u64,
    /// Whether the execution is in sandboxed mode.
    ///
    /// In sandboxed mode, the contract cannot make state changes, call other contracts, emit
    /// messages, etc. No gas is charged for the execution.
    pub sandboxed: bool,
    /// Runtime native config.
    pub runtime_native_config: RuntimeNativeConfig,
    /// Authorization keys for this execution.
    pub authorization_keys: BTreeSet<AccountHash>,
    /// Shared execution stack across nested calls.
    pub execution_stack: Arc<RwLock<VecDeque<ExecutionKind>>>,
}

/// Builder for `ExecuteRequest`.
#[derive(Default)]
pub struct ExecuteRequestBuilder {
    initiator: Option<AccountHash>,
    caller_key: Option<Key>,
    gas_limit: Option<u64>,
    target: Option<ExecutionKind>,
    input: Option<Bytes>,
    value: Option<u64>,
    transaction_hash: Option<TransactionHash>,
    address_generator: Option<Arc<RwLock<AddressGenerator>>>,
    chain_name: Option<Arc<str>>,
    block_time: Option<BlockTime>,
    state_hash: Option<Digest>,
    parent_block_hash: Option<BlockHash>,
    block_height: Option<u64>,
    sandboxed: Option<bool>,
    runtime_native_config: Option<RuntimeNativeConfig>,
    authorization_keys: Option<BTreeSet<AccountHash>>,
    execution_stack: Option<Arc<RwLock<VecDeque<ExecutionKind>>>>,
}

impl ExecuteRequestBuilder {
    /// Set the initiator's address.
    #[must_use]
    pub fn with_initiator(mut self, initiator: AccountHash) -> Self {
        self.initiator = Some(initiator);
        self
    }

    /// Set the caller's key.
    #[must_use]
    pub fn with_caller_key(mut self, caller_key: Key) -> Self {
        self.caller_key = Some(caller_key);
        self
    }

    /// Set the gas limit.
    #[must_use]
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Set the target for execution.
    #[must_use]
    pub fn with_execution_kind(mut self, target: ExecutionKind) -> Self {
        self.target = Some(target);
        self
    }

    /// Pass input data.
    #[must_use]
    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    /// Pass input data that can be serialized.
    pub fn with_serialized_input<T: BorshSerialize>(self, input: T) -> Result<Self, ExecuteError> {
        let input = borsh::to_vec(&input)
            .map(Bytes::from)
            .map_err(|_| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
        Ok(self.with_input(input))
    }

    /// Pass value to be sent to the contract.
    #[must_use]
    pub fn with_transferred_value(mut self, value: u64) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the transaction hash.
    #[must_use]
    pub fn with_transaction_hash(mut self, transaction_hash: TransactionHash) -> Self {
        self.transaction_hash = Some(transaction_hash);
        self
    }

    /// Set the address generator.
    ///
    /// This can be either seeded and created as part of the builder or shared across chain of
    /// execution requests.
    #[must_use]
    pub fn with_address_generator(mut self, address_generator: AddressGenerator) -> Self {
        self.address_generator = Some(Arc::new(RwLock::new(address_generator)));
        self
    }

    /// Set the shared address generator.
    ///
    /// This is useful when the address generator is shared across a chain of multiple execution
    /// requests.
    #[must_use]
    pub fn with_shared_address_generator(
        mut self,
        address_generator: Arc<RwLock<AddressGenerator>>,
    ) -> Self {
        self.address_generator = Some(address_generator);
        self
    }

    /// Set the chain name.
    #[must_use]
    pub fn with_chain_name<T: Into<Arc<str>>>(mut self, chain_name: T) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Set the block time.
    #[must_use]
    pub fn with_block_time(mut self, block_time: BlockTime) -> Self {
        self.block_time = Some(block_time);
        self
    }

    /// Set the state hash.
    #[must_use]
    pub fn with_state_hash(mut self, state_hash: Digest) -> Self {
        self.state_hash = Some(state_hash);
        self
    }

    /// Set the parent block hash.
    #[must_use]
    pub fn with_parent_block_hash(mut self, parent_block_hash: BlockHash) -> Self {
        self.parent_block_hash = Some(parent_block_hash);
        self
    }

    /// Set the block height.
    #[must_use]
    pub fn with_block_height(mut self, block_height: u64) -> Self {
        self.block_height = Some(block_height);
        self
    }

    /// Set the sandboxed mode.
    #[must_use]
    pub fn with_sandboxed(mut self, sandboxed: bool) -> Self {
        self.sandboxed = Some(sandboxed);
        self
    }

    /// Set the runtime native config.
    pub fn with_runtime_native_config(
        mut self,
        runtime_native_config: RuntimeNativeConfig,
    ) -> Self {
        self.runtime_native_config = Some(runtime_native_config);
        self
    }

    /// Set the authorization keys.
    pub fn with_authorization_keys(mut self, authorization_keys: BTreeSet<AccountHash>) -> Self {
        self.authorization_keys = Some(authorization_keys);
        self
    }

    /// Set the shared execution stack used to track nested calls.
    pub fn with_execution_stack(
        mut self,
        execution_stack: Arc<RwLock<VecDeque<ExecutionKind>>>,
    ) -> Self {
        self.execution_stack = Some(execution_stack);
        self
    }

    /// Build the `ExecuteRequest`.
    pub fn build(self) -> Result<ExecuteRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator is not set")?;
        let caller_key = self.caller_key.ok_or("Caller is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let execution_kind = self.target.ok_or("Target is not set")?;
        let input = self.input.unwrap_or_default();
        let transferred_value = self.value.unwrap_or_default();
        let transaction_hash = self.transaction_hash.ok_or("Transaction hash is not set")?;
        let address_generator = self
            .address_generator
            .ok_or("Address generator is not set")?;
        let chain_name = self.chain_name.unwrap_or(Arc::from("casper-test"));
        let block_time = self.block_time.unwrap_or_default();
        let state_hash = self.state_hash.ok_or("State hash is not set")?;
        let parent_block_hash = self
            .parent_block_hash
            .ok_or("Parent block hash is not set")?;
        let block_height = self.block_height.unwrap_or_default();
        let sandboxed = self.sandboxed.unwrap_or(false);
        let runtime_native_config = self
            .runtime_native_config
            .ok_or("Runtime native config not set")?;
        let authorization_keys = self
            .authorization_keys
            .ok_or("Authorization keys are not set")?;
        let execution_stack = self
            .execution_stack
            .unwrap_or_else(|| Arc::new(RwLock::new(VecDeque::new())));
        Ok(ExecuteRequest {
            initiator,
            caller_key,
            gas_limit,
            execution_kind,
            input,
            transferred_value,
            transaction_hash,
            address_generator,
            chain_name,
            block_time,
            state_hash,
            parent_block_hash,
            block_height,
            sandboxed,
            runtime_native_config,
            authorization_keys,
            execution_stack,
        })
    }
}

/// Result of executing a Wasm contract.
#[derive(Debug)]
pub struct ExecuteResult {
    /// Error while executing Wasm: traps, memory access errors, etc.
    pub host_error: Option<CallError>,
    /// Output produced by the Wasm contract.
    pub output: Option<Bytes>,
    /// Gas usage.
    pub gas_usage: GasUsage,
    /// Effects produced by the execution.
    pub effects: Effects,
    /// Cache of tracking copy effects produced by the execution.
    pub cache: TrackingCopyCache,
    /// Messages produced by the execution.
    pub messages: Messages,
}

impl ExecuteResult {
    /// Returns the host error.
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    pub fn into_effects(self) -> Effects {
        self.effects
    }

    pub fn host_error(&self) -> Option<&CallError> {
        self.host_error.as_ref()
    }

    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }
}

/// Result of executing a Wasm contract on a state provider.
#[derive(Debug)]
pub struct ExecuteWithProviderResult {
    /// Error while executing Wasm: traps, memory access errors, etc.
    pub host_error: Option<CallError>,
    /// Output produced by the Wasm contract.
    output: Option<Bytes>,
    /// Gas usage.
    gas_usage: GasUsage,
    /// Effects produced by the execution.
    effects: Effects,
    /// Post state hash.
    post_state_hash: Digest,
    /// Messages produced by the execution.
    messages: Messages,
}

impl ExecuteWithProviderResult {
    #[must_use]
    pub fn new(
        host_error: Option<CallError>,
        output: Option<Bytes>,
        gas_usage: GasUsage,
        effects: Effects,
        post_state_hash: Digest,
        messages: Messages,
    ) -> Self {
        Self {
            host_error,
            output,
            gas_usage,
            effects,
            post_state_hash,
            messages,
        }
    }

    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }

    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    #[must_use]
    pub fn post_state_hash(&self) -> Digest {
        self.post_state_hash
    }

    pub fn messages(&self) -> &Messages {
        &self.messages
    }
}

/// Available options for interacting with the emitting functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitMethods {
    PrintStd,
    Native,
}

/// Available options for interacting with functions manipulating
/// and fetching data from global state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalStateMethods {
    Read,
    Write,
    Remove,
    GetBalance,
    GetInfo,
    Create,
}

/// Available options for interacting with functions interacting
/// with other contracts and control flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMethods {
    Call,
    Upgrade,
}

/// Available options for interacting with the system mint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintMethods {
    Burn,
    Transfer,
    TransferPurse,
}

/// Available options for interacting with the system auction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuctionMethods {
    Activate,
    Bid,
    Withdraw,
    Delegate,
    Undelegate,
    Redelegate,
    AddReservation,
    CancelReservation,
    ChangePublicKey,
}

/// Available options for interacting with host-side cryptographic functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoMethods {
    AltBn128Add,
    AltBn128Multiply,
    AltBn128Pairing,
    GenericHash,
    RecoverSecp256K1,
}

/// Available options for interacting with host-side cryptographic functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IOMethods {
    Return,
    CopyInput,
}

/// Specific subsection of FFIMenu actions that will be executed as system contract calls
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemContractMenu {
    Mint(MintMethods),
    Auction(AuctionMethods),
}

/// Available options for interacting with the host ffi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FFIMenu {
    Mint(MintMethods),
    Auction(AuctionMethods),
    Crypto(CryptoMethods),
    Emit(EmitMethods),
    GlobalState(GlobalStateMethods),
    Control(ControlMethods),
    IO(IOMethods),
}

impl FFIMenu {
    pub fn allowed_in_sandbox(&self) -> bool {
        match self {
            FFIMenu::Mint(mint_methods) => match mint_methods {
                MintMethods::Burn => false,
                MintMethods::Transfer => false,
                MintMethods::TransferPurse => false,
            },
            FFIMenu::Auction(auction_methods) => match auction_methods {
                AuctionMethods::Activate => false,
                AuctionMethods::Bid => false,
                AuctionMethods::Withdraw => false,
                AuctionMethods::Delegate => false,
                AuctionMethods::Undelegate => false,
                AuctionMethods::Redelegate => false,
                AuctionMethods::AddReservation => false,
                AuctionMethods::CancelReservation => false,
                AuctionMethods::ChangePublicKey => false,
            },
            FFIMenu::Crypto(crypto_methods) => match crypto_methods {
                CryptoMethods::AltBn128Add => true,
                CryptoMethods::AltBn128Multiply => true,
                CryptoMethods::AltBn128Pairing => true,
                CryptoMethods::GenericHash => true,
                CryptoMethods::RecoverSecp256K1 => true,
            },
            FFIMenu::Emit(emit_methods) => match emit_methods {
                EmitMethods::PrintStd => true,
                EmitMethods::Native => false,
            },
            FFIMenu::GlobalState(global_state_methods) => match global_state_methods {
                GlobalStateMethods::Read => true,
                GlobalStateMethods::Write => false,
                GlobalStateMethods::Remove => false,
                GlobalStateMethods::GetBalance => true,
                GlobalStateMethods::GetInfo => true,
                GlobalStateMethods::Create => false,
            },
            FFIMenu::Control(control_methods) => match control_methods {
                ControlMethods::Call => false,
                ControlMethods::Upgrade => false,
            },
            FFIMenu::IO(iomethods) => match iomethods {
                IOMethods::Return => true,
                IOMethods::CopyInput => true,
            },
        }
    }

    pub fn all_ffi_options() -> impl Iterator<Item = FFIMenu> {
        FFIPrimitiveValue::iter().map(|raw| FFIMenu::from(&raw))
    }
}

impl TryFrom<u32> for FFIMenu {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match FFIPrimitiveValue::from_u32(value) {
            Some(primitive) => Ok((&primitive).into()),
            None => Err(()),
        }
    }
}

impl From<FFIMenu> for u32 {
    fn from(value: FFIMenu) -> u32 {
        FFIPrimitiveValue::from(&value) as u32
    }
}

/// Target for Wasm execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionKind {
    /// Execute Wasm bytes directly.
    SessionBytes(Bytes),
    /// Execute a stored contract by its address.
    Stored {
        /// Address of the contract's package.
        address: HashAddr,
        /// Entry point to call.
        entry_point: String,
    },
    /// Interact with the system.
    System(SystemContractMenu),
}

impl ExecutionKind {
    /// Returns system menu selection if relevant.
    pub fn ffi_selection(&self) -> Option<SystemContractMenu> {
        match self {
            ExecutionKind::SessionBytes(_) | ExecutionKind::Stored { .. } => None,
            ExecutionKind::System(menu) => Some(menu.clone()),
        }
    }
}

/// Error that can occur during execution, before the Wasm virtual machine is involved.
///
/// This error is returned by the `execute` function. It contains information about the error that
/// occurred.
#[derive(Debug, Error)]
pub enum ExecuteError {
    /// Error while preparing Wasm instance: export not found, validation, compilation errors, etc.
    ///
    /// No wasm was executed at this point.
    #[error("Wasm error error: {0}")]
    WasmPreparation(#[from] WasmPreparationError),
    /// Error while executing Wasm: traps, memory access errors, etc.
    #[error("Internal host error: {0}")]
    Fatal(#[from] FatalHostError),
    #[error("Argument size ({argument_size}) exceeds VM memory limit ({memory_limit})")]
    ArgumentSizeExceedsMemory {
        argument_size: usize,
        memory_limit: u32,
    },
    // Wasm attempted to return flags that are not supported
    #[error("Return flags are not supported: {0}")]
    ReturnFlagsNotSupported(u32),
    #[error("Api error: {0}")]
    Api(String),
    #[error("sandboxed system contract call")]
    SandboxedSystemContractCall,
    #[error("unable to find main purse for v2 contract {0}")]
    MainPurseNotFound(Key),
    #[error("unable to convert key into uref {0}")]
    InvalidKeyForPurse(Key),
    #[error("attempt to call a non-existent ffi option {0}")]
    InvalidFFIOption(u32),
    #[error("attempted writing in restricted mode")]
    AttemptWriteInRestricted,
}

#[derive(Debug, Error)]
pub enum ExecuteWithProviderError {
    /// Error while accessing global state.
    #[error("Global state error: {0}")]
    GlobalState(#[from] GlobalStateError),
    #[error(transparent)]
    Execute(#[from] ExecuteError),
}

/// Executor trait.
///
/// An executor is responsible for executing Wasm contracts. This implies that the executor is able
/// to prepare Wasm instances, execute them, and handle errors that occur during execution.
///
/// Trait bounds also implying that the executor has to support interior mutability, as it may need
/// to update its internal state during execution of a single or a chain of multiple contracts.
pub trait Executor: Clone + Send {
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError>;

    /// Execute a contract in sandboxed mode.
    ///
    /// This method executes a smart contract in a sandbox that cannot call or message outward,
    /// or mutate state.
    fn execute_sandbox<R: GlobalStateReader + 'static>(
        &self,
        tracking_copy: TrackingCopy<R>,
        runtime_native_config: RuntimeNativeConfig,
        request: SandboxedExecutionRequest,
    ) -> Result<SandboxedExecutionResult, ExecuteError>;
}

#[repr(u32)]
#[derive(EnumIter, FromPrimitive)]
enum FFIPrimitiveValue {
    /* Mint values */
    MintTransfer = 0,
    MintTransferPurse = 1,
    MintBurn = 2,
    /* Auction values */
    AuctionActivate = 100,
    AuctionBid = 101,
    AuctionWithdraw = 102,
    AuctionDelegate = 103,
    AuctionUndelegate = 104,
    AuctionRedelegate = 105,
    AuctionAddReservation = 106,
    AuctionCancelReservation = 107,
    AuctionChangePublicKey = 108,
    /* Crypto values */
    CryptoAltBn128Add = 200,
    CryptoAltBn128Multiply = 201,
    CryptoAltBn128Pairing = 202,
    CryptoGenericHash = 203,
    CryptoRecoverSecp256K1 = 204,
    /* Emit values */
    EmitPrintStd = 300,
    EmitNative = 301,
    /* Global state values */
    GlobalStateRead = 400,
    GlobalStateWrite = 401,
    GlobalStateRemove = 402,
    GlobalStateGetBalance = 403,
    GlobalStateGetInfo = 404,
    GlobalStateCreate = 405,
    /* Control values */
    ControlCall = 500,
    ControlUpgrade = 501,
    /* IO values */
    IOReturn = 600,
    IOCopyInput = 601,
}

impl From<&FFIPrimitiveValue> for FFIMenu {
    fn from(value: &FFIPrimitiveValue) -> Self {
        match value {
            FFIPrimitiveValue::MintTransfer => Self::Mint(MintMethods::Transfer),
            FFIPrimitiveValue::MintTransferPurse => Self::Mint(MintMethods::TransferPurse),
            FFIPrimitiveValue::MintBurn => Self::Mint(MintMethods::Burn),
            FFIPrimitiveValue::AuctionActivate => Self::Auction(AuctionMethods::Activate),
            FFIPrimitiveValue::AuctionBid => Self::Auction(AuctionMethods::Bid),
            FFIPrimitiveValue::AuctionWithdraw => Self::Auction(AuctionMethods::Withdraw),
            FFIPrimitiveValue::AuctionDelegate => Self::Auction(AuctionMethods::Delegate),
            FFIPrimitiveValue::AuctionUndelegate => Self::Auction(AuctionMethods::Undelegate),
            FFIPrimitiveValue::AuctionRedelegate => Self::Auction(AuctionMethods::Redelegate),
            FFIPrimitiveValue::AuctionAddReservation => {
                Self::Auction(AuctionMethods::AddReservation)
            }
            FFIPrimitiveValue::AuctionCancelReservation => {
                Self::Auction(AuctionMethods::CancelReservation)
            }
            FFIPrimitiveValue::AuctionChangePublicKey => {
                Self::Auction(AuctionMethods::ChangePublicKey)
            }
            FFIPrimitiveValue::CryptoAltBn128Add => Self::Crypto(CryptoMethods::AltBn128Add),
            FFIPrimitiveValue::CryptoAltBn128Multiply => {
                Self::Crypto(CryptoMethods::AltBn128Multiply)
            }
            FFIPrimitiveValue::CryptoAltBn128Pairing => {
                Self::Crypto(CryptoMethods::AltBn128Pairing)
            }
            FFIPrimitiveValue::CryptoGenericHash => Self::Crypto(CryptoMethods::GenericHash),
            FFIPrimitiveValue::CryptoRecoverSecp256K1 => {
                Self::Crypto(CryptoMethods::RecoverSecp256K1)
            }
            FFIPrimitiveValue::EmitPrintStd => Self::Emit(EmitMethods::PrintStd),
            FFIPrimitiveValue::EmitNative => Self::Emit(EmitMethods::Native),
            FFIPrimitiveValue::GlobalStateRead => Self::GlobalState(GlobalStateMethods::Read),
            FFIPrimitiveValue::GlobalStateWrite => Self::GlobalState(GlobalStateMethods::Write),
            FFIPrimitiveValue::GlobalStateRemove => Self::GlobalState(GlobalStateMethods::Remove),
            FFIPrimitiveValue::GlobalStateGetBalance => {
                Self::GlobalState(GlobalStateMethods::GetBalance)
            }
            FFIPrimitiveValue::GlobalStateGetInfo => Self::GlobalState(GlobalStateMethods::GetInfo),
            FFIPrimitiveValue::GlobalStateCreate => Self::GlobalState(GlobalStateMethods::Create),
            FFIPrimitiveValue::ControlCall => Self::Control(ControlMethods::Call),
            FFIPrimitiveValue::ControlUpgrade => Self::Control(ControlMethods::Upgrade),
            FFIPrimitiveValue::IOReturn => Self::IO(IOMethods::Return),
            FFIPrimitiveValue::IOCopyInput => Self::IO(IOMethods::CopyInput),
        }
    }
}

impl From<&FFIMenu> for FFIPrimitiveValue {
    fn from(value: &FFIMenu) -> Self {
        match value {
            FFIMenu::Mint(mint_methods) => match mint_methods {
                MintMethods::Burn => Self::MintBurn,
                MintMethods::Transfer => Self::MintTransfer,
                MintMethods::TransferPurse => Self::MintTransferPurse,
            },
            FFIMenu::Auction(auction_methods) => match auction_methods {
                AuctionMethods::Activate => Self::AuctionActivate,
                AuctionMethods::Bid => Self::AuctionBid,
                AuctionMethods::Withdraw => Self::AuctionWithdraw,
                AuctionMethods::Delegate => Self::AuctionDelegate,
                AuctionMethods::Undelegate => Self::AuctionUndelegate,
                AuctionMethods::Redelegate => Self::AuctionRedelegate,
                AuctionMethods::AddReservation => Self::AuctionAddReservation,
                AuctionMethods::CancelReservation => Self::AuctionCancelReservation,
                AuctionMethods::ChangePublicKey => Self::AuctionChangePublicKey,
            },
            FFIMenu::Crypto(crypto_methods) => match crypto_methods {
                CryptoMethods::AltBn128Add => Self::CryptoAltBn128Add,
                CryptoMethods::AltBn128Multiply => Self::CryptoAltBn128Multiply,
                CryptoMethods::AltBn128Pairing => Self::CryptoAltBn128Pairing,
                CryptoMethods::GenericHash => Self::CryptoGenericHash,
                CryptoMethods::RecoverSecp256K1 => Self::CryptoRecoverSecp256K1,
            },
            FFIMenu::Emit(emit_methods) => match emit_methods {
                EmitMethods::PrintStd => Self::EmitPrintStd,
                EmitMethods::Native => Self::EmitNative,
            },
            FFIMenu::GlobalState(global_state_methods) => match global_state_methods {
                GlobalStateMethods::Read => Self::GlobalStateRead,
                GlobalStateMethods::Write => Self::GlobalStateWrite,
                GlobalStateMethods::Remove => Self::GlobalStateRemove,
                GlobalStateMethods::GetBalance => Self::GlobalStateGetBalance,
                GlobalStateMethods::GetInfo => Self::GlobalStateGetInfo,
                GlobalStateMethods::Create => Self::GlobalStateCreate,
            },
            FFIMenu::Control(control_methods) => match control_methods {
                ControlMethods::Call => Self::ControlCall,
                ControlMethods::Upgrade => Self::ControlUpgrade,
            },
            FFIMenu::IO(iomethods) => match iomethods {
                IOMethods::Return => Self::IOReturn,
                IOMethods::CopyInput => Self::IOCopyInput,
            },
        }
    }
}
