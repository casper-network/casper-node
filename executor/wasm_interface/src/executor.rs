use std::sync::Arc;

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
use parking_lot::RwLock;
use thiserror::Error;

use crate::{
    CallError, GasUsage, InternalHostError, SandboxedExecutionRequest, SandboxedExecutionResult,
    WasmPreparationError,
};

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
            .map_err(|_| ExecuteError::InternalHost(InternalHostError::TypeConversion))?;
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

    /// Build the `ExecuteRequest`.
    pub fn build(self) -> Result<ExecuteRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator is not set")?;
        let caller_key = self.caller_key.ok_or("Caller is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let execution_kind = self.target.ok_or("Target is not set")?;
        let input = self.input.ok_or("Input is not set")?;
        let transferred_value = self.value.unwrap_or_default();
        let transaction_hash = self.transaction_hash.ok_or("Transaction hash is not set")?;
        let address_generator = self
            .address_generator
            .ok_or("Address generator is not set")?;
        let chain_name = self.chain_name.ok_or("Chain name is not set")?;
        let block_time = self.block_time.ok_or("Block time is not set")?;
        let state_hash = self.state_hash.ok_or("State hash is not set")?;
        let parent_block_hash = self
            .parent_block_hash
            .ok_or("Parent block hash is not set")?;
        let block_height = self.block_height.ok_or("Block height is not set")?;
        let sandboxed = self.sandboxed.unwrap_or(false);
        let runtime_native_config = self
            .runtime_native_config
            .ok_or("Runtime native config not set")?;
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

/// Available options for interacting with the system mint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintMethods {
    Burn,
    Transfer,
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

/// Available options for interacting with the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemMenu {
    Mint(MintMethods),
    Auction(AuctionMethods),
}

impl TryFrom<u32> for SystemMenu {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 0 {
            Ok(SystemMenu::Mint(MintMethods::Transfer))
        } else if value == 1 {
            Ok(SystemMenu::Mint(MintMethods::Burn))
        } else if value == 100 {
            Ok(SystemMenu::Auction(AuctionMethods::Activate))
        } else if value == 101 {
            Ok(SystemMenu::Auction(AuctionMethods::Bid))
        } else if value == 102 {
            Ok(SystemMenu::Auction(AuctionMethods::Withdraw))
        } else if value == 103 {
            Ok(SystemMenu::Auction(AuctionMethods::Delegate))
        } else if value == 104 {
            Ok(SystemMenu::Auction(AuctionMethods::Undelegate))
        } else if value == 105 {
            Ok(SystemMenu::Auction(AuctionMethods::Redelegate))
        } else if value == 106 {
            Ok(SystemMenu::Auction(AuctionMethods::AddReservation))
        } else if value == 107 {
            Ok(SystemMenu::Auction(AuctionMethods::CancelReservation))
        } else if value == 108 {
            Ok(SystemMenu::Auction(AuctionMethods::ChangePublicKey))
        } else {
            Err(())
        }
    }
}

impl From<SystemMenu> for u32 {
    fn from(value: SystemMenu) -> u32 {
        match value {
            SystemMenu::Mint(mint) => match mint {
                MintMethods::Transfer => 0,
                MintMethods::Burn => 1,
            },
            SystemMenu::Auction(auction) => match auction {
                AuctionMethods::Activate => 100,
                AuctionMethods::Bid => 101,
                AuctionMethods::Withdraw => 102,
                AuctionMethods::Delegate => 103,
                AuctionMethods::Undelegate => 104,
                AuctionMethods::Redelegate => 105,
                AuctionMethods::AddReservation => 106,
                AuctionMethods::CancelReservation => 107,
                AuctionMethods::ChangePublicKey => 108,
            },
        }
    }
}

/// Target for Wasm execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionKind {
    /// Execute Wasm bytes directly.
    SessionBytes(Bytes),
    /// Execute a stored contract by its address.
    Stored {
        /// Address of the contract.
        address: HashAddr,
        /// Entry point to call.
        entry_point: String,
    },
    /// Interact with the system.
    System(SystemMenu),
}

impl ExecutionKind {
    /// Returns system menu selection if relevant.
    pub fn system_menu_selection(&self) -> Option<SystemMenu> {
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
    InternalHost(#[from] InternalHostError),
    #[error("Code not found: {0:?}")]
    CodeNotFound(HashAddr),
    #[error("Argument size ({argument_size}) exceeds VM memory limit ({memory_limit})")]
    ArgumentSizeExceedsMemory {
        argument_size: usize,
        memory_limit: u32,
    },
    // Wasm attempted to return flags that are not supported
    #[error("Return flags are not supported: {0}")]
    ReturnFlagsNotSupported(u32),
    #[error("Entity not found: {0}")]
    EntityNotFound(Key),
    #[error("No active contract found in smart contract package: {0}")]
    NoActiveContract(Key),
    #[error("Api error: {0}")]
    Api(String),
    #[error("sandboxed system contract call")]
    SandboxedSystemContractCall,
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
