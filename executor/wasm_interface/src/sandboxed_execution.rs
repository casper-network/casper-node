use casper_types::{
    account::AccountHash, bytesrepr::Bytes as BytesreprBytes, contract_messages::Messages,
    execution::Effects, BlockHash, BlockTime, ContractRuntimeTag, Digest, Gas, ProtocolVersion,
    RuntimeArgs, TransactionArgs, TransactionEntryPoint, TransactionTarget, Transfer,
};
use std::collections::BTreeSet;

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
    /// Input invalid
    InputInvalid,
    /// V1 execution engine error
    V1EngineError(String),
    /// System is callee and signaled vm instance kill.
    Revert(String),
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
            SandboxedExecutionError::InputInvalid => write!(f, "input invalid"),
            SandboxedExecutionError::V1EngineError(engine_error) => {
                write!(f, "V1 execution error: {}", engine_error)
            }
            SandboxedExecutionError::Revert(revert_error) => write!(f, "{}", revert_error),
        }
    }
}

/// A request to execute a sandboxed contract. Pure functions, read-only getters, beacons,
/// sentinels, and similar functionality that does not require invocation of other contracts or
/// mutation of state are supported.
#[derive(Debug, PartialEq)]
pub struct SandboxedExecutionRequest {
    /// Identifier of the block at which global state the execution should happen
    pub state_hash: Digest,
    /// Height of the block
    pub block_height: u64,
    /// Block time
    pub block_time: BlockTime,
    /// Parent hash
    pub parent_block_hash: BlockHash,
    /// Protocol version
    pub protocol_version: ProtocolVersion,
    /// gas limit
    pub gas_limit: Gas,
    /// Transaction target
    pub target: TransactionTarget,
    /// Entry point
    pub entry_point: TransactionEntryPoint,
    /// The address of the account that would initiate the contract call.
    pub initiator: AccountHash,
    /// Input data for the query.
    pub args: TransactionArgs,
    /// Authorization keys
    pub authorization_keys: BTreeSet<AccountHash>,
}

impl SandboxedExecutionRequest {
    pub fn get_contract_runtime_tag(&self) -> Option<ContractRuntimeTag> {
        match &self.target {
            TransactionTarget::Native => None,
            TransactionTarget::Stored { id: _, runtime } => Some(runtime.contract_runtime_tag()),
            TransactionTarget::Session {
                is_install_upgrade: _,
                module_bytes: _,
                runtime,
            } => Some(runtime.contract_runtime_tag()),
        }
    }

    pub fn entry_point_name(&self) -> String {
        match &self.entry_point {
            TransactionEntryPoint::Custom(x) => x.to_string(),
            _ => "call".to_string(),
        }
    }

    pub fn rutime_args(&self) -> Option<RuntimeArgs> {
        match &self.args {
            TransactionArgs::Named(runtime_args) => Some(runtime_args.clone()),
            TransactionArgs::Bytesrepr(_) => None,
        }
    }
}

/// Wrapper struct on the results of executing a transaction
/// in sandboxed mode
#[derive(Debug)]
pub struct SuccessfullResultOutput {
    /// bytes output from the executions
    pub output: BytesreprBytes,
    /// List of transfers that happened during execution.
    pub transfers: Vec<Transfer>,
    /// Gas limit.
    pub limit: Gas,
    /// Gas consumed.
    pub consumed: Gas,
    /// Execution effects.
    pub effects: Effects,
    /// Messages emitted during execution.
    pub messages: Messages,
}

/// Result of a sandboxed execution.
#[derive(Debug)]
pub struct SandboxedExecutionResult {
    /// Error while executing, if any.
    pub error: Option<SandboxedExecutionError>,
    /// Output data returned by the contract.
    pub output: Option<SuccessfullResultOutput>,
    /// Gas usage tracked during execution.
    pub gas_usage: Gas,
}

impl SandboxedExecutionResult {
    /// Returns the error if the execution failed.
    pub fn error(&self) -> Option<&SandboxedExecutionError> {
        self.error.as_ref()
    }

    /// Returns the output data if the execution succeeded.
    pub fn output(&self) -> Option<&BytesreprBytes> {
        self.output.as_ref().map(|x| &x.output)
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
    state_hash: Option<Digest>,
    block_height: Option<u64>,
    block_time: Option<BlockTime>,
    parent_block_hash: Option<BlockHash>,
    protocol_version: Option<ProtocolVersion>,
    target: Option<TransactionTarget>,
    entry_point: Option<TransactionEntryPoint>,
    gas_limit: Option<Gas>,
    args: Option<TransactionArgs>,
    authorization_keys: Option<BTreeSet<AccountHash>>,
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
    pub fn with_target(mut self, target: TransactionTarget) -> Self {
        self.target = Some(target);
        self
    }

    /// Set the entry point to call.
    #[must_use]
    pub fn with_entry_point(mut self, entry_point: TransactionEntryPoint) -> Self {
        self.entry_point = Some(entry_point);
        self
    }

    /// Set the input data.
    #[must_use]
    pub fn with_args(mut self, args: TransactionArgs) -> Self {
        self.args = Some(args);
        self
    }

    /// Build the `SandboxedExecutionRequest`.
    pub fn build(self) -> Result<SandboxedExecutionRequest, &'static str> {
        let initiator = self.initiator.ok_or("initiator is not set")?;
        let state_hash = self.state_hash.ok_or("state_hash is not set")?;
        let block_height = self.block_height.ok_or("block_height is not set")?;
        let block_time = self.block_time.ok_or("block_time is not set")?;
        let parent_block_hash = self
            .parent_block_hash
            .ok_or("parent_block_hash is not set")?;
        let protocol_version = self.protocol_version.ok_or("protocol_version is not set")?;
        let target = self.target.ok_or("target is not set")?;
        let entry_point = self.entry_point.ok_or("entry_point is not set")?;
        let args = self.args.ok_or("args is not set")?;
        let gas_limit = self.gas_limit.ok_or("gas_limit is not set")?;
        let authorization_keys = self
            .authorization_keys
            .ok_or("authorization_keys is not set")?;
        Ok(SandboxedExecutionRequest {
            state_hash,
            block_height,
            block_time,
            parent_block_hash,
            protocol_version,
            target,
            entry_point,
            initiator,
            args,
            gas_limit,
            authorization_keys,
        })
    }
}
