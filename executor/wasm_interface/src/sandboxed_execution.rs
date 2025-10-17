use casper_types::{
    account::AccountHash,
    bytesrepr,
    bytesrepr::{Bytes, FromBytes, ToBytes},
    BlockHash, BlockTime, Digest, Gas, HashAddr,
};

/// Errors that can occur during sandboxed execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxedExecutionError {
    /// The contract rolled back execution for the callee.
    CalleeRolledBack,
    /// The contract trapped during execution.
    CalleeTrapped,
    /// The contract ran out of gas.
    CalleeGasDepleted,
    /// The contract is not callable (missing export).
    NotCallable,
    /// The contract code was not found.
    CodeNotFound,
    /// An internal host error occurred.
    InternalHostError,
    /// No active contract in a package.
    NoActiveContract,
    /// Entity not found
    EntityNotFound,
    /// Tried to upgrade a contract in a locked package.
    LockedPackage,
    /// Api error occurred.
    Api(String),
    /// Input invalid
    InputInvalid,
}

impl core::fmt::Display for SandboxedExecutionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SandboxedExecutionError::CalleeRolledBack => write!(f, "contract rolled back"),
            SandboxedExecutionError::CalleeTrapped => write!(f, "contract trapped"),
            SandboxedExecutionError::CalleeGasDepleted => write!(f, "contract gas depleted"),
            SandboxedExecutionError::NotCallable => write!(f, "contract not callable"),
            SandboxedExecutionError::CodeNotFound => write!(f, "contract code not found"),
            SandboxedExecutionError::InternalHostError => write!(f, "internal host error"),
            SandboxedExecutionError::NoActiveContract => write!(f, "no active contract"),
            SandboxedExecutionError::EntityNotFound => write!(f, "entity not found"),
            SandboxedExecutionError::LockedPackage => write!(f, "locked package"),
            SandboxedExecutionError::Api(api_error) => write!(f, "{}", api_error),
            SandboxedExecutionError::InputInvalid => write!(f, "input invalid"),
        }
    }
}

/// A request to execute a sandboxed contract. Pure functions, read-only getters, beacons,
/// sentinels, and similar functionality that does not require invocation of other contracts or
/// mutation of state are supported.
#[derive(Debug, PartialEq)]
pub struct SandboxedExecutionRequest {
    /// The address of the account that would initiate the contract call.
    pub initiator: AccountHash,
    /// The address of the contract to query.
    pub contract_address: HashAddr,
    /// The entry point to call.
    pub entry_point: String,
    /// Input data for the query.
    pub input: Bytes,
    /// Gas limit for the query execution.
    ///
    /// This prevents infinite loops and resource exhaustion attacks.
    /// The caller is not charged actual tokens, but must provide a limit
    /// to protect against malicious contracts that could stall the node.
    pub gas_limit: u64,
    /// Block time for the query context.
    pub block_time: BlockTime,
    /// State root hash to query against.
    pub state_hash: Digest,
    /// Parent block hash for context.
    pub parent_block_hash: BlockHash,
    /// Block height for context.
    pub block_height: u64,
    /// Chain name for context.
    pub chain_name: String,
}

impl ToBytes for SandboxedExecutionRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut writer = bytesrepr::allocate_buffer(self)?;
        self.initiator.write_bytes(&mut writer)?;
        self.contract_address.write_bytes(&mut writer)?;
        self.entry_point.write_bytes(&mut writer)?;
        self.input.write_bytes(&mut writer)?;
        self.gas_limit.write_bytes(&mut writer)?;
        self.block_time.write_bytes(&mut writer)?;
        self.state_hash.write_bytes(&mut writer)?;
        self.parent_block_hash.write_bytes(&mut writer)?;
        self.block_height.write_bytes(&mut writer)?;
        self.chain_name.write_bytes(&mut writer)?;
        Ok(writer)
    }

    fn serialized_length(&self) -> usize {
        self.initiator.serialized_length()
            + self.contract_address.serialized_length()
            + self.entry_point.serialized_length()
            + self.input.serialized_length()
            + self.gas_limit.serialized_length()
            + self.block_time.serialized_length()
            + self.state_hash.serialized_length()
            + self.parent_block_hash.serialized_length()
            + self.block_height.serialized_length()
            + self.chain_name.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.initiator.write_bytes(writer)?;
        self.contract_address.write_bytes(writer)?;
        self.entry_point.write_bytes(writer)?;
        self.input.write_bytes(writer)?;
        self.gas_limit.write_bytes(writer)?;
        self.block_time.write_bytes(writer)?;
        self.state_hash.write_bytes(writer)?;
        self.parent_block_hash.write_bytes(writer)?;
        self.block_height.write_bytes(writer)?;
        self.chain_name.write_bytes(writer)
    }
}

impl FromBytes for SandboxedExecutionRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (initiator, remainder) = FromBytes::from_bytes(bytes)?;
        let (contract_address, remainder) = FromBytes::from_bytes(remainder)?;
        let (entry_point, remainder) = FromBytes::from_bytes(remainder)?;
        let (input, remainder) = FromBytes::from_bytes(remainder)?;
        let (gas_limit, remainder) = FromBytes::from_bytes(remainder)?;
        let (block_time, remainder) = FromBytes::from_bytes(remainder)?;
        let (state_hash, remainder) = FromBytes::from_bytes(remainder)?;
        let (parent_block_hash, remainder) = FromBytes::from_bytes(remainder)?;
        let (block_height, remainder) = FromBytes::from_bytes(remainder)?;
        let (chain_name, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            SandboxedExecutionRequest {
                initiator,
                contract_address,
                entry_point,
                input,
                gas_limit,
                block_time,
                state_hash,
                parent_block_hash,
                block_height,
                chain_name,
            },
            remainder,
        ))
    }
}

/// Result of a sandboxed execution.
#[derive(Debug)]
pub struct SandboxedExecutionResult {
    /// Error while executing, if any.
    pub error: Option<SandboxedExecutionError>,
    /// Output data returned by the contract.
    pub output: Option<Bytes>,
    /// Gas usage tracked during execution.
    pub gas_usage: Gas,
}

impl SandboxedExecutionResult {
    /// Returns the error if the execution failed.
    pub fn error(&self) -> Option<&SandboxedExecutionError> {
        self.error.as_ref()
    }

    /// Returns the output data if the execution succeeded.
    pub fn output(&self) -> Option<&Bytes> {
        self.output.as_ref()
    }

    /// Returns the gas spent.
    pub fn gas_usage(&self) -> &Gas {
        &self.gas_usage
    }

    /// Returns true if the query was successful.
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// Builder for `SandboxedExecutionRequest`.
#[derive(Default)]
pub struct SandboxedExecutionRequestBuilder {
    initiator: Option<AccountHash>,
    contract_address: Option<HashAddr>,
    entry_point: Option<String>,
    input: Option<Bytes>,
    gas_limit: Option<u64>,
    block_time: Option<BlockTime>,
    state_hash: Option<Digest>,
    parent_block_hash: Option<BlockHash>,
    block_height: Option<u64>,
    chain_name: Option<String>,
}

impl SandboxedExecutionRequestBuilder {
    /// Set the initiator's address.
    #[must_use]
    pub fn with_initiator(mut self, initiator: AccountHash) -> Self {
        self.initiator = Some(initiator);
        self
    }

    /// Set the contract address to query.
    #[must_use]
    pub fn with_contract_address(mut self, contract_address: HashAddr) -> Self {
        self.contract_address = Some(contract_address);
        self
    }

    /// Set the entry point to call.
    #[must_use]
    pub fn with_entry_point(mut self, entry_point: String) -> Self {
        self.entry_point = Some(entry_point);
        self
    }

    /// Set the input data.
    #[must_use]
    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    /// Set the gas limit.
    #[must_use]
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
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

    /// Set the chain name.
    #[must_use]
    pub fn with_chain_name<T: Into<String>>(mut self, chain_name: T) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Build the `SandboxedExecutionRequest`.
    pub fn build(self) -> Result<SandboxedExecutionRequest, &'static str> {
        let initiator = self.initiator.ok_or("Initiator is not set")?;
        let contract_address = self.contract_address.ok_or("Contract address is not set")?;
        let entry_point = self.entry_point.ok_or("Entry point is not set")?;
        let input = self.input.ok_or("Input is not set")?;
        let gas_limit = self.gas_limit.ok_or("Gas limit is not set")?;
        let block_time = self.block_time.ok_or("Block time is not set")?;
        let state_hash = self.state_hash.ok_or("State hash is not set")?;
        let parent_block_hash = self
            .parent_block_hash
            .ok_or("Parent block hash is not set")?;
        let block_height = self.block_height.ok_or("Block height is not set")?;
        let chain_name = self.chain_name.ok_or("Chain name is not set")?;
        Ok(SandboxedExecutionRequest {
            initiator,
            contract_address,
            entry_point,
            input,
            gas_limit,
            block_time,
            state_hash,
            parent_block_hash,
            block_height,
            chain_name,
        })
    }
}
