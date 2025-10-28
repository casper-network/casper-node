use casper_types::{
    account::AccountHash,
    bytesrepr,
    bytesrepr::{Bytes, FromBytes, ToBytes},
    BlockHash, BlockTime, Digest, Gas, HashAddr,
};
use core::convert::TryFrom;

/// Errors that can occur during sandboxed execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxedExecutionError {
    /// The contract rolled back execution.
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

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum SandboxedExecutionErrorTag {
    CalleeRolledBack = 0,
    CalleeTrapped = 1,
    CalleeGasDepleted = 2,
    NotCallable = 3,
    CodeNotFound = 4,
    InternalHostError = 5,
    NoActiveContract = 6,
    EntityNotFound = 7,
    LockedPackage = 8,
    Api = 9,
    InputInvalid = 10,
}

impl TryFrom<u8> for SandboxedExecutionErrorTag {
    type Error = bytesrepr::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            x if x == SandboxedExecutionErrorTag::CalleeRolledBack as u8 => {
                Ok(SandboxedExecutionErrorTag::CalleeRolledBack)
            }
            x if x == SandboxedExecutionErrorTag::CalleeTrapped as u8 => {
                Ok(SandboxedExecutionErrorTag::CalleeTrapped)
            }
            x if x == SandboxedExecutionErrorTag::CalleeGasDepleted as u8 => {
                Ok(SandboxedExecutionErrorTag::CalleeGasDepleted)
            }
            x if x == SandboxedExecutionErrorTag::NotCallable as u8 => {
                Ok(SandboxedExecutionErrorTag::NotCallable)
            }
            x if x == SandboxedExecutionErrorTag::CodeNotFound as u8 => {
                Ok(SandboxedExecutionErrorTag::CodeNotFound)
            }
            x if x == SandboxedExecutionErrorTag::InternalHostError as u8 => {
                Ok(SandboxedExecutionErrorTag::InternalHostError)
            }
            x if x == SandboxedExecutionErrorTag::NoActiveContract as u8 => {
                Ok(SandboxedExecutionErrorTag::NoActiveContract)
            }
            x if x == SandboxedExecutionErrorTag::EntityNotFound as u8 => {
                Ok(SandboxedExecutionErrorTag::EntityNotFound)
            }
            x if x == SandboxedExecutionErrorTag::LockedPackage as u8 => {
                Ok(SandboxedExecutionErrorTag::LockedPackage)
            }
            x if x == SandboxedExecutionErrorTag::Api as u8 => Ok(SandboxedExecutionErrorTag::Api),
            x if x == SandboxedExecutionErrorTag::InputInvalid as u8 => {
                Ok(SandboxedExecutionErrorTag::InputInvalid)
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl SandboxedExecutionError {
    fn tag(&self) -> SandboxedExecutionErrorTag {
        match self {
            SandboxedExecutionError::CalleeRolledBack => {
                SandboxedExecutionErrorTag::CalleeRolledBack
            }
            SandboxedExecutionError::CalleeTrapped => SandboxedExecutionErrorTag::CalleeTrapped,
            SandboxedExecutionError::CalleeGasDepleted => {
                SandboxedExecutionErrorTag::CalleeGasDepleted
            }
            SandboxedExecutionError::NotCallable => SandboxedExecutionErrorTag::NotCallable,
            SandboxedExecutionError::CodeNotFound => SandboxedExecutionErrorTag::CodeNotFound,
            SandboxedExecutionError::InternalHostError => {
                SandboxedExecutionErrorTag::InternalHostError
            }
            SandboxedExecutionError::NoActiveContract => {
                SandboxedExecutionErrorTag::NoActiveContract
            }
            SandboxedExecutionError::EntityNotFound => SandboxedExecutionErrorTag::EntityNotFound,
            SandboxedExecutionError::LockedPackage => SandboxedExecutionErrorTag::LockedPackage,
            SandboxedExecutionError::Api(_) => SandboxedExecutionErrorTag::Api,
            SandboxedExecutionError::InputInvalid => SandboxedExecutionErrorTag::InputInvalid,
        }
    }
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

#[cfg(test)]
impl SandboxedExecutionRequest {
    /// Generates a random request for testing.
    pub fn random(rng: &mut casper_types::testing::TestRng) -> Self {
        use rand::Rng;

        SandboxedExecutionRequest {
            initiator: AccountHash::new(rng.gen()),
            contract_address: rng.gen(),
            entry_point: format!("entry_point_{}", rng.gen::<u32>()),
            input: vec![rng.gen::<u8>(); 32].into(),
            gas_limit: rng.gen_range(1000..1000000),
            block_time: BlockTime::new(rng.gen()),
            state_hash: Digest::random(rng),
            parent_block_hash: BlockHash::random(rng),
            block_height: rng.gen(),
            chain_name: String::default(),
        }
    }
}

/// Result of a sandboxed execution.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl ToBytes for SandboxedExecutionError {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut writer = bytesrepr::allocate_buffer(self)?;
        let tag: u8 = self.tag() as u8;
        tag.write_bytes(&mut writer)?;
        if let SandboxedExecutionError::Api(msg) = self {
            msg.write_bytes(&mut writer)?;
        }
        Ok(writer)
    }

    fn serialized_length(&self) -> usize {
        let base = bytesrepr::U8_SERIALIZED_LENGTH; // tag
        match self {
            SandboxedExecutionError::Api(msg) => base + msg.serialized_length(),
            _ => base,
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend(self.to_bytes()?);
        Ok(())
    }
}

impl FromBytes for SandboxedExecutionError {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag_u8, remainder) = u8::from_bytes(bytes)?;
        let tag = SandboxedExecutionErrorTag::try_from(tag_u8)?;
        match tag {
            SandboxedExecutionErrorTag::CalleeRolledBack => {
                Ok((SandboxedExecutionError::CalleeRolledBack, remainder))
            }
            SandboxedExecutionErrorTag::CalleeTrapped => {
                Ok((SandboxedExecutionError::CalleeTrapped, remainder))
            }
            SandboxedExecutionErrorTag::CalleeGasDepleted => {
                Ok((SandboxedExecutionError::CalleeGasDepleted, remainder))
            }
            SandboxedExecutionErrorTag::NotCallable => {
                Ok((SandboxedExecutionError::NotCallable, remainder))
            }
            SandboxedExecutionErrorTag::CodeNotFound => {
                Ok((SandboxedExecutionError::CodeNotFound, remainder))
            }
            SandboxedExecutionErrorTag::InternalHostError => {
                Ok((SandboxedExecutionError::InternalHostError, remainder))
            }
            SandboxedExecutionErrorTag::NoActiveContract => {
                Ok((SandboxedExecutionError::NoActiveContract, remainder))
            }
            SandboxedExecutionErrorTag::EntityNotFound => {
                Ok((SandboxedExecutionError::EntityNotFound, remainder))
            }
            SandboxedExecutionErrorTag::LockedPackage => {
                Ok((SandboxedExecutionError::LockedPackage, remainder))
            }
            SandboxedExecutionErrorTag::Api => {
                let (msg, rem) = String::from_bytes(remainder)?;
                Ok((SandboxedExecutionError::Api(msg), rem))
            }
            SandboxedExecutionErrorTag::InputInvalid => {
                Ok((SandboxedExecutionError::InputInvalid, remainder))
            }
        }
    }
}

impl ToBytes for SandboxedExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut writer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut writer)?;
        Ok(writer)
    }

    fn serialized_length(&self) -> usize {
        self.error.serialized_length()
            + self.output.serialized_length()
            + self.gas_usage.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.error.write_bytes(writer)?;
        self.output.write_bytes(writer)?;
        self.gas_usage.write_bytes(writer)
    }
}

impl FromBytes for SandboxedExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (error, bytes) = Option::<SandboxedExecutionError>::from_bytes(bytes)?;
        let (output, bytes) = Option::<Bytes>::from_bytes(bytes)?;
        let (gas_usage, bytes) = Gas::from_bytes(bytes)?;
        Ok((
            SandboxedExecutionResult {
                error,
                output,
                gas_usage,
            },
            bytes,
        ))
    }
}
