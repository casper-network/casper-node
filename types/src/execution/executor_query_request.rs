use alloc::{string::String, vec::Vec};

use crate::{
    account::AccountHash,
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    BlockHash, BlockTime, Digest, Gas, HashAddr,
};

/// A request to execute a read-only query on a contract.
///
/// This allows off-chain querying of contract state without making global state changes
/// or costing gas. A gas limit must be provided to prevent infinite loops and resource
/// exhaustion attacks.
#[derive(Debug, PartialEq)]
pub struct VmQueryRequest {
    /// The address of the account that would initiate the contract call.
    pub initiator: AccountHash,
    /// The address of the contract to query.
    pub contract_address: HashAddr,
    /// The entry point to call.
    pub entry_point: String,
    /// Input data for the query.
    pub input: Vec<u8>,
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

impl ToBytes for VmQueryRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut writer = bytesrepr::allocate_buffer(self)?;
        self.state_hash.write_bytes(&mut writer)?;
        self.contract_address.write_bytes(&mut writer)?;
        self.entry_point.write_bytes(&mut writer)?;
        self.input.write_bytes(&mut writer)?;
        self.gas_limit.write_bytes(&mut writer)?;
        Ok(writer)
    }

    fn serialized_length(&self) -> usize {
        self.state_hash.serialized_length()
            + self.contract_address.serialized_length()
            + self.entry_point.serialized_length()
            + self.input.serialized_length()
            + self.gas_limit.serialized_length()
    }
}

impl FromBytes for VmQueryRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (state_hash, remainder) = FromBytes::from_bytes(bytes)?;
        let (contract_address, remainder) = FromBytes::from_bytes(remainder)?;
        let (entry_point, remainder) = FromBytes::from_bytes(remainder)?;
        let (input, remainder) = FromBytes::from_bytes(remainder)?;
        let (gas_limit, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            VmQueryRequest {
                initiator: AccountHash::default(),
                contract_address,
                entry_point,
                input,
                gas_limit,
                block_time: BlockTime::default(),
                state_hash,
                parent_block_hash: BlockHash::default(),
                block_height: 0,
                chain_name: String::default(),
            },
            remainder,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl VmQueryRequest {
    pub fn random(rng: &mut crate::testing::TestRng) -> Self {
        use rand::Rng;

        VmQueryRequest {
            initiator: AccountHash::new(rng.gen()),
            contract_address: rng.gen(),
            entry_point: format!("entry_point_{}", rng.gen::<u32>()),
            input: vec![rng.gen::<u8>(); 32],
            gas_limit: rng.gen_range(1000..1000000),
            block_time: BlockTime::new(rng.gen()),
            state_hash: Digest::random(rng),
            parent_block_hash: BlockHash::random(rng),
            block_height: rng.gen(),
            chain_name: String::default(),
        }
    }
}

/// Errors that can occur during query execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The contract reverted execution.
    CalleeReverted,
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
}

impl core::fmt::Display for QueryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            QueryError::CalleeReverted => write!(f, "contract reverted"),
            QueryError::CalleeTrapped => write!(f, "contract trapped"),
            QueryError::CalleeGasDepleted => write!(f, "contract gas depleted"),
            QueryError::NotCallable => write!(f, "contract not callable"),
            QueryError::CodeNotFound => write!(f, "contract code not found"),
            QueryError::InternalHostError => write!(f, "internal host error"),
        }
    }
}

/// Result of executing a read-only query.
#[derive(Debug)]
pub struct ExecutorQueryResult {
    /// Error while executing the query, if any.
    pub error: Option<QueryError>,
    /// Output data returned by the contract.
    pub output: Option<Bytes>,
    /// Gas usage tracked during execution. Use `gas_spent()` to get the gas consumed.
    pub gas_usage: Gas,
}

impl ExecutorQueryResult {
    /// Returns the error if the query failed.
    pub fn error(&self) -> Option<&QueryError> {
        self.error.as_ref()
    }

    /// Returns the output data if the query succeeded.
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

/// Builder for `QueryRequest`.
#[derive(Default)]
pub struct ExecutorQueryRequestBuilder {
    initiator: Option<AccountHash>,
    contract_address: Option<HashAddr>,
    entry_point: Option<String>,
    input: Option<Vec<u8>>,
    gas_limit: Option<u64>,
    block_time: Option<BlockTime>,
    state_hash: Option<Digest>,
    parent_block_hash: Option<BlockHash>,
    block_height: Option<u64>,
    chain_name: Option<String>,
}

impl ExecutorQueryRequestBuilder {
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
    pub fn with_input(mut self, input: Vec<u8>) -> Self {
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

    /// Build the `QueryRequest`.
    pub fn build(self) -> Result<VmQueryRequest, &'static str> {
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
        Ok(VmQueryRequest {
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
