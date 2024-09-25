use std::sync::Arc;

use bytes::Bytes;
use casper_executor_wasm_interface::{executor::ExecuteError, GasUsage, HostError};
use casper_storage::{global_state::error::Error as GlobalStateError, AddressGenerator};
use casper_types::{
    account::AccountHash,
    contracts::{ContractHash, ContractPackageHash},
    execution::Effects,
    Digest, Timestamp, TransactionHash,
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
    pub(crate) transferred_value: u128,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Address generator.
    pub(crate) address_generator: Arc<RwLock<AddressGenerator>>,
    /// Chain name.
    pub(crate) chain_name: Arc<str>,
    /// Block time.
    pub(crate) block_time: Timestamp,
}

#[derive(Default)]
pub struct InstallContractRequestBuilder {
    initiator: Option<AccountHash>,
    gas_limit: Option<u64>,
    wasm_bytes: Option<Bytes>,
    entry_point: Option<String>,
    input: Option<Bytes>,
    transferred_value: Option<u128>,
    transaction_hash: Option<TransactionHash>,
    address_generator: Option<Arc<RwLock<AddressGenerator>>>,
    chain_name: Option<Arc<str>>,
    block_time: Option<Timestamp>,
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

    pub fn with_transferred_value(mut self, transferred_value: u128) -> Self {
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

    pub fn with_block_time(mut self, block_time: Timestamp) -> Self {
        self.block_time = Some(block_time);
        self
    }

    pub fn build(self) -> Result<InstallContractRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit not set")?;
        let wasm_bytes = self.wasm_bytes.ok_or("Wasm bytes not set")?;
        let entry_point = self.entry_point;
        let input = self.input;
        let transferred_value = self.transferred_value.ok_or("Value not set")?;
        let address_generator = self.address_generator.ok_or("Address generator not set")?;
        let transaction_hash = self.transaction_hash.ok_or("Transaction hash not set")?;
        let chain_name = self.chain_name.ok_or("Chain name not set")?;
        let block_time = self.block_time.ok_or("Block time not set")?;
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
        })
    }
}

/// Result of executing a Wasm contract.
#[derive(Debug)]
pub struct InstallContractResult {
    /// Contract package hash.
    pub(crate) contract_package_hash: ContractPackageHash,
    /// Contract hash.
    pub(crate) contract_hash: ContractHash,
    /// Version
    pub(crate) version: u32,
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

    pub fn contract_package_hash(&self) -> &ContractPackageHash {
        &self.contract_package_hash
    }

    pub fn contract_hash(&self) -> &ContractHash {
        &self.contract_hash
    }

    pub fn version(&self) -> &u32 {
        &self.version
    }

    pub fn gas_usage(&self) -> &GasUsage {
        &self.gas_usage
    }

    pub fn post_state_hash(&self) -> Digest {
        self.post_state_hash
    }
}

#[derive(Debug, Error)]
pub enum InstallContractError {
    #[error("system contract error: {0}")]
    SystemContract(HostError),

    #[error("execute: {0}")]
    Execute(ExecuteError),

    #[error("Global state error: {0}")]
    GlobalState(#[from] GlobalStateError),

    #[error("constructor error: {host_error}")]
    Constructor { host_error: HostError },
}
