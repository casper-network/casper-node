use std::{collections::BTreeSet, sync::Arc};

use bytes::Bytes;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::{executor::ExecuteError, GasUsage};
use casper_storage::{
    global_state::error::Error as GlobalStateError, AddressGenerator, RuntimeNativeConfig,
};
use casper_types::{
    account::AccountHash, execution::Effects, BlockHash, BlockTime, CLValueError, Digest,
    TransactionHash,
};
use parking_lot::RwLock;
use thiserror::Error;

// NOTE: One struct that represents both InstallContractRequest and ExecuteRequest.

/// Store contract request.
pub struct InstallContractRequest {
    /// Initiator's address.
    pub(crate) initiator: AccountHash,
    /// Gas limit.
    pub(crate) gas_limit: u64,
    /// Wasm bytes of the contract to be stored.
    pub(crate) wasm_bytes: Bytes,
    /// Constructor entry point name.
    pub(crate) entry_point: Option<String>,
    /// Input data for the constructor.
    pub(crate) input: Option<Bytes>,
    /// Attached tokens value that to be transferred into the constructor.
    pub(crate) transferred_value: u64,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Address generator.
    pub(crate) address_generator: Arc<RwLock<AddressGenerator>>,
    /// Chain name.
    pub(crate) chain_name: Arc<str>,
    /// Block time.
    pub(crate) block_time: BlockTime,
    /// State hash.
    pub(crate) state_hash: Digest,
    /// Parent block hash.
    pub(crate) parent_block_hash: BlockHash,
    /// Block height.
    pub(crate) block_height: u64,
    /// Seed used for smart contract hash computation.
    pub(crate) seed: Option<[u8; 32]>,
    /// Runtime native config.
    pub(crate) runtime_native_config: RuntimeNativeConfig,
    /// Authorization keys for this installation.
    pub(crate) authorization_keys: BTreeSet<AccountHash>,
}

#[derive(Default)]
pub struct InstallContractRequestBuilder {
    initiator: Option<AccountHash>,
    gas_limit: Option<u64>,
    wasm_bytes: Option<Bytes>,
    entry_point: Option<String>,
    input: Option<Bytes>,
    transferred_value: Option<u64>,
    transaction_hash: Option<TransactionHash>,
    address_generator: Option<Arc<RwLock<AddressGenerator>>>,
    chain_name: Option<Arc<str>>,
    block_time: Option<BlockTime>,
    state_hash: Option<Digest>,
    parent_block_hash: Option<BlockHash>,
    block_height: Option<u64>,
    runtime_native_config: Option<RuntimeNativeConfig>,
    seed: Option<[u8; 32]>,
    authorization_keys: Option<BTreeSet<AccountHash>>,
}

impl InstallContractRequestBuilder {
    pub fn with_initiator(mut self, initiator: AccountHash) -> Self {
        self.initiator = Some(initiator);
        self
    }

    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    pub fn with_wasm_bytes(mut self, wasm_bytes: Bytes) -> Self {
        self.wasm_bytes = Some(wasm_bytes);
        self
    }

    pub fn with_entry_point(mut self, entry_point: String) -> Self {
        self.entry_point = Some(entry_point);
        self
    }

    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    pub fn with_transferred_value(mut self, transferred_value: u64) -> Self {
        self.transferred_value = Some(transferred_value);
        self
    }

    pub fn with_address_generator(mut self, address_generator: AddressGenerator) -> Self {
        self.address_generator = Some(Arc::new(RwLock::new(address_generator)));
        self
    }

    pub fn with_shared_address_generator(
        mut self,
        address_generator: Arc<RwLock<AddressGenerator>>,
    ) -> Self {
        self.address_generator = Some(address_generator);
        self
    }

    pub fn with_transaction_hash(mut self, transaction_hash: TransactionHash) -> Self {
        self.transaction_hash = Some(transaction_hash);
        self
    }

    pub fn with_chain_name<T: Into<Arc<str>>>(mut self, chain_name: T) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    pub fn with_block_time(mut self, block_time: BlockTime) -> Self {
        self.block_time = Some(block_time);
        self
    }

    pub fn with_seed(mut self, seed: [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn with_state_hash(mut self, state_hash: Digest) -> Self {
        self.state_hash = Some(state_hash);
        self
    }

    pub fn with_parent_block_hash(mut self, parent_block_hash: BlockHash) -> Self {
        self.parent_block_hash = Some(parent_block_hash);
        self
    }

    pub fn with_block_height(mut self, block_height: u64) -> Self {
        self.block_height = Some(block_height);
        self
    }

    pub fn with_runtime_native_config(
        mut self,
        runtime_native_config: RuntimeNativeConfig,
    ) -> Self {
        self.runtime_native_config = Some(runtime_native_config);
        self
    }

    pub fn with_authorization_keys(mut self, authorization_keys: BTreeSet<AccountHash>) -> Self {
        self.authorization_keys = Some(authorization_keys);
        self
    }

    pub fn build(self) -> Result<InstallContractRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit not set")?;
        let wasm_bytes = self.wasm_bytes.ok_or("Wasm bytes not set")?;
        let entry_point = self.entry_point;
        let input = self.input;
        let transferred_value = self.transferred_value.unwrap_or_default();
        let address_generator = self.address_generator.ok_or("Address generator not set")?;
        let transaction_hash = self.transaction_hash.ok_or("Transaction hash not set")?;
        let chain_name = self.chain_name.unwrap_or(Arc::from("casper-test"));
        let block_time = self.block_time.unwrap_or_default();
        let seed = self.seed;
        let state_hash = self.state_hash.ok_or("State hash not set")?;
        let parent_block_hash = self.parent_block_hash.ok_or("Parent block hash not set")?;
        let block_height = self.block_height.unwrap_or_default();
        let runtime_native_config = self
            .runtime_native_config
            .ok_or("Runtime native config not set")?;
        let authorization_keys = self
            .authorization_keys
            .ok_or("Authorization keys not set")?;
        Ok(InstallContractRequest {
            initiator,
            gas_limit,
            wasm_bytes,
            entry_point,
            input,
            transferred_value,
            address_generator,
            transaction_hash,
            chain_name,
            block_time,
            seed,
            state_hash,
            parent_block_hash,
            block_height,
            runtime_native_config,
            authorization_keys,
        })
    }
}

/// Result of executing a Wasm contract.
#[derive(Debug)]
pub struct InstallContractResult {
    /// Smart contract address.
    pub(crate) smart_contract_addr: [u8; 32],
    /// Gas usage.
    pub(crate) gas_usage: GasUsage,
    /// Effects produced by the execution.
    pub(crate) effects: Effects,
    /// Post state hash after installation.
    pub(crate) post_state_hash: Digest,
}

impl InstallContractResult {
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }

    pub fn post_state_hash(&self) -> Digest {
        self.post_state_hash
    }

    pub fn smart_contract_addr(&self) -> &[u8; 32] {
        &self.smart_contract_addr
    }
}

#[derive(Debug, Error)]
pub enum InstallContractError {
    #[error("system contract error: {0}")]
    SystemContract(CallError),

    #[error("execute: {0}")]
    Execute(ExecuteError),

    #[error("Global state error: {0}")]
    GlobalState(#[from] GlobalStateError),

    #[error("constructor error: {host_error}")]
    Constructor { host_error: CallError },

    #[error("failed building BuildingExecuteRequest: {0}")]
    FailedBuildingExecuteRequest(&'static str),

    #[error("CLValue error: {0}")]
    CLValueError(CLValueError),
}
