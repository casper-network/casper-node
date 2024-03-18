use casper_types::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    contract_messages::Messages,
    execution::Effects,
    BlockTime, Digest, Gas, Phase, ProtocolVersion, Transaction, TransferAddr,
};

use crate::engine_state::{Error as EngineError, ExecutionResult};

/// Wasm v1 request.
pub struct WasmV1Request {
    pub(super) state_hash: Digest,
    pub(super) protocol_version: ProtocolVersion,
    pub(super) block_time: BlockTime,
    pub(super) transaction: Transaction,
    pub(super) gas_limit: Gas,
    pub(super) phase: Phase,
}

impl WasmV1Request {
    /// Returns new instance of WasmV1Request.
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: BlockTime,
        transaction: Transaction,
        gas_limit: Gas,
        phase: Phase,
    ) -> Self {
        WasmV1Request {
            state_hash,
            protocol_version,
            block_time,
            transaction,
            gas_limit,
            phase,
        }
    }

    /// State hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Blocktime.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }

    /// Transaction.
    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    /// Gas limit.
    pub fn gas_limit(&self) -> Gas {
        self.gas_limit
    }

    /// Phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

impl ToBytes for WasmV1Request {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.state_hash)
            + ToBytes::serialized_length(&self.protocol_version)
            + ToBytes::serialized_length(&self.block_time)
            + ToBytes::serialized_length(&self.protocol_version)
            + ToBytes::serialized_length(&self.transaction)
            + ToBytes::serialized_length(&self.gas_limit)
            + ToBytes::serialized_length(&self.phase)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.state_hash.write_bytes(writer)?;
        self.protocol_version.write_bytes(writer)?;
        self.block_time.write_bytes(writer)?;
        self.transaction.write_bytes(writer)?;
        self.gas_limit.write_bytes(writer)?;
        self.phase.write_bytes(writer)
    }
}

impl FromBytes for WasmV1Request {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (state_hash, bytes) = Digest::from_bytes(bytes)?;
        let (protocol_version, bytes) = ProtocolVersion::from_bytes(bytes)?;
        let (blocktime, bytes) = BlockTime::from_bytes(bytes)?;
        let (transaction, bytes) = Transaction::from_bytes(bytes)?;
        let (gas_limit, bytes) = Gas::from_bytes(bytes)?;
        let (phase, bytes) = Phase::from_bytes(bytes)?;
        Ok((
            WasmV1Request {
                state_hash,
                protocol_version,
                block_time: blocktime,
                transaction,
                gas_limit,
                phase,
            },
            bytes,
        ))
    }
}

/// Wasm v1 result.
#[derive(Debug)]
pub struct WasmV1Result {
    /// List of transfers that happened during execution.
    transfers: Vec<TransferAddr>,
    /// Gas limit.
    limit: Gas,
    /// Gas consumed.
    consumed: Gas,
    /// Execution effects.
    effects: Effects,
    /// Messages emitted during execution.
    messages: Messages,
    /// Did the wasm execute successfully?
    error: Option<EngineError>,
}

impl WasmV1Result {
    /// Error, if any.
    pub fn error(&self) -> &Option<EngineError> {
        &self.error
    }

    /// List of transfers that happened during execution.
    pub fn transfers(&self) -> &Vec<TransferAddr> {
        &self.transfers
    }

    /// Gas limit.
    pub fn limit(&self) -> Gas {
        self.limit
    }

    /// Gas consumed.
    pub fn consumed(&self) -> Gas {
        self.consumed
    }

    /// Execution effects.
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Messages emitted during execution.
    pub fn messages(&self) -> &Messages {
        &self.messages
    }

    /// Root not found.
    pub fn root_not_found(gas_limit: Gas, state_hash: Digest) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(EngineError::RootNotFound(state_hash)),
        }
    }

    /// Precondition failure.
    pub fn precondition_failure(gas_limit: Gas, error: EngineError) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(error),
        }
    }

    /// Failed to transform transaction into an executable item.
    pub fn invalid_executable_item(gas_limit: Gas) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(EngineError::InvalidExecutableItem),
        }
    }

    /// From an execution result.
    pub fn execution_result(gas_limit: Gas, execution_result: ExecutionResult) -> Self {
        match execution_result {
            ExecutionResult::Failure {
                error,
                transfers,
                gas,
                effects,
                messages,
            } => WasmV1Result {
                error: Some(error),
                transfers,
                effects,
                messages,
                limit: gas_limit,
                consumed: gas,
            },
            ExecutionResult::Success {
                transfers,
                gas,
                effects,
                messages,
            } => WasmV1Result {
                transfers,
                effects,
                messages,
                limit: gas_limit,
                consumed: gas,
                error: None,
            },
        }
    }
}
