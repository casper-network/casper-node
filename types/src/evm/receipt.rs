//! EVM transaction receipt types.

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Address, Hash};
use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// High-level status recorded in an EVM transaction receipt.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum ReceiptStatus {
    /// EVM execution completed successfully.
    Success,
    /// EVM execution reverted and returned revert bytes.
    Revert,
    /// EVM execution halted for an exceptional reason.
    Halt(HaltReason),
}

impl ReceiptStatus {
    fn tag(self) -> u8 {
        match self {
            ReceiptStatus::Success => 0,
            ReceiptStatus::Revert => 1,
            ReceiptStatus::Halt(_) => 2,
        }
    }

    /// Returns the binary status expected by `eth_getTransactionReceipt`.
    pub fn eth_status(self) -> u8 {
        match self {
            ReceiptStatus::Success => 1,
            ReceiptStatus::Revert | ReceiptStatus::Halt(_) => 0,
        }
    }

    /// Returns `true` when the receipt represents successful EVM execution.
    pub fn is_success(self) -> bool {
        matches!(self, ReceiptStatus::Success)
    }

    /// Returns a stable diagnostic message derived from the typed status.
    pub fn message(self) -> Option<&'static str> {
        match self {
            ReceiptStatus::Success => None,
            ReceiptStatus::Revert => Some("EVM reverted"),
            ReceiptStatus::Halt(reason) => Some(reason.message()),
        }
    }
}

impl ToBytes for ReceiptStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ReceiptStatus::Success | ReceiptStatus::Revert => 0,
                ReceiptStatus::Halt(reason) => reason.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        if let ReceiptStatus::Halt(reason) = self {
            reason.write_bytes(writer)?;
        }
        Ok(())
    }
}

impl FromBytes for ReceiptStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let status = match tag {
            0 => ReceiptStatus::Success,
            1 => ReceiptStatus::Revert,
            2 => {
                let (reason, remainder) = HaltReason::from_bytes(remainder)?;
                return Ok((ReceiptStatus::Halt(reason), remainder));
            }
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((status, remainder))
    }
}

/// Reason an EVM execution halted exceptionally.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum HaltReason {
    /// Execution ran out of gas.
    OutOfGas(OutOfGasError),
    /// The bytecode contained an unknown opcode.
    OpcodeNotFound,
    /// The bytecode executed the invalid `0xFE` opcode.
    InvalidFEOpcode,
    /// Execution jumped to an invalid destination.
    InvalidJump,
    /// The opcode or feature is not active for the configured hardfork.
    NotActivated,
    /// Execution attempted to pop a value from an empty stack.
    StackUnderflow,
    /// Execution attempted to push a value onto a full stack.
    StackOverflow,
    /// Execution used an invalid memory or storage offset.
    OutOfOffset,
    /// Contract creation collided with an existing account.
    CreateCollision,
    /// A precompile failed.
    PrecompileError,
    /// Account nonce overflowed.
    NonceOverflow,
    /// Created contract runtime bytecode exceeded the configured limit.
    CreateContractSizeLimit,
    /// Created contract runtime bytecode starts with `0xEF`.
    CreateContractStartingWithEF,
    /// Contract init code exceeded the configured limit.
    CreateInitCodeSizeLimit,
    /// Payment accounting overflowed.
    OverflowPayment,
    /// Execution attempted a state change during a static call.
    StateChangeDuringStaticCall,
    /// Execution attempted a call disallowed during a static call.
    CallNotAllowedInsideStatic,
    /// The caller did not have enough funds.
    OutOfFunds,
    /// Call depth exceeded the EVM limit.
    CallTooDeep,
    /// Halt reason was not recognized by this version.
    Unknown,
}

impl HaltReason {
    fn tag(self) -> u8 {
        match self {
            HaltReason::OutOfGas(_) => 0,
            HaltReason::OpcodeNotFound => 1,
            HaltReason::InvalidFEOpcode => 2,
            HaltReason::InvalidJump => 3,
            HaltReason::NotActivated => 4,
            HaltReason::StackUnderflow => 5,
            HaltReason::StackOverflow => 6,
            HaltReason::OutOfOffset => 7,
            HaltReason::CreateCollision => 8,
            HaltReason::PrecompileError => 9,
            HaltReason::NonceOverflow => 10,
            HaltReason::CreateContractSizeLimit => 11,
            HaltReason::CreateContractStartingWithEF => 12,
            HaltReason::CreateInitCodeSizeLimit => 13,
            HaltReason::OverflowPayment => 14,
            HaltReason::StateChangeDuringStaticCall => 15,
            HaltReason::CallNotAllowedInsideStatic => 16,
            HaltReason::OutOfFunds => 17,
            HaltReason::CallTooDeep => 18,
            HaltReason::Unknown => 19,
        }
    }

    /// Returns a stable diagnostic message for this halt reason.
    pub fn message(self) -> &'static str {
        match self {
            HaltReason::OutOfGas(reason) => reason.message(),
            HaltReason::OpcodeNotFound => "EVM halted: opcode not found",
            HaltReason::InvalidFEOpcode => "EVM halted: invalid 0xFE opcode",
            HaltReason::InvalidJump => "EVM halted: invalid jump destination",
            HaltReason::NotActivated => "EVM halted: feature or opcode not activated",
            HaltReason::StackUnderflow => "EVM halted: stack underflow",
            HaltReason::StackOverflow => "EVM halted: stack overflow",
            HaltReason::OutOfOffset => "EVM halted: out of offset",
            HaltReason::CreateCollision => "EVM halted: create collision",
            HaltReason::PrecompileError => "EVM halted: precompile error",
            HaltReason::NonceOverflow => "EVM halted: nonce overflow",
            HaltReason::CreateContractSizeLimit => "EVM halted: create contract size limit",
            HaltReason::CreateContractStartingWithEF => {
                "EVM halted: create contract starting with 0xEF"
            }
            HaltReason::CreateInitCodeSizeLimit => "EVM halted: create initcode size limit",
            HaltReason::OverflowPayment => "EVM halted: overflow payment",
            HaltReason::StateChangeDuringStaticCall => {
                "EVM halted: state change during static call"
            }
            HaltReason::CallNotAllowedInsideStatic => {
                "EVM halted: call not allowed inside static call"
            }
            HaltReason::OutOfFunds => "EVM halted: out of funds",
            HaltReason::CallTooDeep => "EVM halted: call too deep",
            HaltReason::Unknown => "EVM halted: unknown reason",
        }
    }

    /// Returns a random EVM halt reason.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..20) {
            0 => HaltReason::OutOfGas(OutOfGasError::random(rng)),
            1 => HaltReason::OpcodeNotFound,
            2 => HaltReason::InvalidFEOpcode,
            3 => HaltReason::InvalidJump,
            4 => HaltReason::NotActivated,
            5 => HaltReason::StackUnderflow,
            6 => HaltReason::StackOverflow,
            7 => HaltReason::OutOfOffset,
            8 => HaltReason::CreateCollision,
            9 => HaltReason::PrecompileError,
            10 => HaltReason::NonceOverflow,
            11 => HaltReason::CreateContractSizeLimit,
            12 => HaltReason::CreateContractStartingWithEF,
            13 => HaltReason::CreateInitCodeSizeLimit,
            14 => HaltReason::OverflowPayment,
            15 => HaltReason::StateChangeDuringStaticCall,
            16 => HaltReason::CallNotAllowedInsideStatic,
            17 => HaltReason::OutOfFunds,
            18 => HaltReason::CallTooDeep,
            19 => HaltReason::Unknown,
            _ => unreachable!(),
        }
    }
}

impl ToBytes for HaltReason {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                HaltReason::OutOfGas(reason) => reason.serialized_length(),
                _ => 0,
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        if let HaltReason::OutOfGas(reason) = self {
            reason.write_bytes(writer)?;
        }
        Ok(())
    }
}

impl FromBytes for HaltReason {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let reason = match tag {
            0 => {
                let (reason, remainder) = OutOfGasError::from_bytes(remainder)?;
                return Ok((HaltReason::OutOfGas(reason), remainder));
            }
            1 => HaltReason::OpcodeNotFound,
            2 => HaltReason::InvalidFEOpcode,
            3 => HaltReason::InvalidJump,
            4 => HaltReason::NotActivated,
            5 => HaltReason::StackUnderflow,
            6 => HaltReason::StackOverflow,
            7 => HaltReason::OutOfOffset,
            8 => HaltReason::CreateCollision,
            9 => HaltReason::PrecompileError,
            10 => HaltReason::NonceOverflow,
            11 => HaltReason::CreateContractSizeLimit,
            12 => HaltReason::CreateContractStartingWithEF,
            13 => HaltReason::CreateInitCodeSizeLimit,
            14 => HaltReason::OverflowPayment,
            15 => HaltReason::StateChangeDuringStaticCall,
            16 => HaltReason::CallNotAllowedInsideStatic,
            17 => HaltReason::OutOfFunds,
            18 => HaltReason::CallTooDeep,
            19 => HaltReason::Unknown,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((reason, remainder))
    }
}

/// Reason execution ran out of gas.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum OutOfGasError {
    /// Not enough gas to execute an opcode.
    Basic,
    /// Memory limit exceeded.
    MemoryLimit,
    /// Memory expansion ran out of gas.
    Memory,
    /// Precompile ran out of gas.
    Precompile,
    /// Operand was too large to fit into the required native type.
    InvalidOperand,
    /// `SSTORE` was attempted with too little gas remaining.
    ReentrancySentry,
}

impl OutOfGasError {
    fn tag(self) -> u8 {
        match self {
            OutOfGasError::Basic => 0,
            OutOfGasError::MemoryLimit => 1,
            OutOfGasError::Memory => 2,
            OutOfGasError::Precompile => 3,
            OutOfGasError::InvalidOperand => 4,
            OutOfGasError::ReentrancySentry => 5,
        }
    }

    /// Returns a stable diagnostic message for this out-of-gas reason.
    pub fn message(self) -> &'static str {
        match self {
            OutOfGasError::Basic => "EVM halted: out of gas",
            OutOfGasError::MemoryLimit => "EVM halted: out of gas: memory limit exceeded",
            OutOfGasError::Memory => "EVM halted: out of gas: memory expansion",
            OutOfGasError::Precompile => "EVM halted: out of gas: precompile",
            OutOfGasError::InvalidOperand => "EVM halted: out of gas: invalid operand",
            OutOfGasError::ReentrancySentry => "EVM halted: out of gas: reentrancy sentry",
        }
    }

    /// Returns a random out-of-gas error.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..6) {
            0 => OutOfGasError::Basic,
            1 => OutOfGasError::MemoryLimit,
            2 => OutOfGasError::Memory,
            3 => OutOfGasError::Precompile,
            4 => OutOfGasError::InvalidOperand,
            5 => OutOfGasError::ReentrancySentry,
            _ => unreachable!(),
        }
    }
}

impl ToBytes for OutOfGasError {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(vec![self.tag()])
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        Ok(())
    }
}

impl FromBytes for OutOfGasError {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let reason = match tag {
            0 => OutOfGasError::Basic,
            1 => OutOfGasError::MemoryLimit,
            2 => OutOfGasError::Memory,
            3 => OutOfGasError::Precompile,
            4 => OutOfGasError::InvalidOperand,
            5 => OutOfGasError::ReentrancySentry,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((reason, remainder))
    }
}

/// EVM log entry emitted by a transaction.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Log {
    /// Contract address that emitted the log.
    pub address: Address,
    /// Indexed log topics.
    pub topics: Vec<Hash>,
    /// Unindexed log data.
    pub data: Bytes,
}

impl ToBytes for Log {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.address.serialized_length()
            + self.topics.serialized_length()
            + self.data.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.address.write_bytes(writer)?;
        self.topics.write_bytes(writer)?;
        self.data.write_bytes(writer)
    }
}

impl FromBytes for Log {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (address, remainder) = Address::from_bytes(bytes)?;
        let (topics, remainder) = Vec::<Hash>::from_bytes(remainder)?;
        let (data, remainder) = Bytes::from_bytes(remainder)?;
        Ok((
            Log {
                address,
                topics,
                data,
            },
            remainder,
        ))
    }
}

/// EVM transaction receipt data persisted with an EVM execution result.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Transaction execution status.
    pub status: ReceiptStatus,
    /// Gas consumed by EVM execution.
    pub gas_used: u64,
    /// Effective gas price used for Ethereum receipt projection.
    pub effective_gas_price: u128,
    /// Contract address created by the transaction, if any.
    pub contract_address: Option<Address>,
    /// Logs emitted by successful execution.
    pub logs: Vec<Log>,
}

impl Receipt {
    /// Returns a random EVM receipt.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let log_count = rng.gen_range(0..4);
        let logs = (0..log_count)
            .map(|_| Log {
                address: Address::new(rng.gen()),
                topics: (0..rng.gen_range(0..4))
                    .map(|_| Hash::new(rng.gen()))
                    .collect(),
                data: Bytes::from({
                    let mut data = vec![0; rng.gen_range(0..16)];
                    rng.fill(data.as_mut_slice());
                    data
                }),
            })
            .collect();
        Receipt {
            status: match rng.gen_range(0..3) {
                0 => ReceiptStatus::Success,
                1 => ReceiptStatus::Revert,
                2 => ReceiptStatus::Halt(HaltReason::random(rng)),
                _ => unreachable!(),
            },
            gas_used: rng.gen(),
            effective_gas_price: rng.gen(),
            contract_address: rng.gen::<bool>().then(|| Address::new(rng.gen())),
            logs,
        }
    }
}

impl ToBytes for Receipt {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.status.serialized_length()
            + self.gas_used.serialized_length()
            + self.effective_gas_price.serialized_length()
            + self.contract_address.serialized_length()
            + self.logs.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.status.write_bytes(writer)?;
        self.gas_used.write_bytes(writer)?;
        self.effective_gas_price.write_bytes(writer)?;
        self.contract_address.write_bytes(writer)?;
        self.logs.write_bytes(writer)
    }
}

impl FromBytes for Receipt {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (status, remainder) = ReceiptStatus::from_bytes(bytes)?;
        let (gas_used, remainder) = u64::from_bytes(remainder)?;
        let (effective_gas_price, remainder) = u128::from_bytes(remainder)?;
        let (contract_address, remainder) = Option::<Address>::from_bytes(remainder)?;
        let (logs, remainder) = Vec::<Log>::from_bytes(remainder)?;
        Ok((
            Receipt {
                status,
                gas_used,
                effective_gas_price,
                contract_address,
                logs,
            },
            remainder,
        ))
    }
}
