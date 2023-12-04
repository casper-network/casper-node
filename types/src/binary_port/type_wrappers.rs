//! Type aliases.

use core::time::Duration;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "datasize")]
use datasize::DataSize;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    contract_messages::Messages,
    execution::ExecutionResultV2,
    EraId, PublicKey, Timestamp, ValidatorChange,
};

/// Type representing uptime.
#[derive(Debug, Clone, Copy)]
pub struct Uptime(pub u64);

impl From<Uptime> for Duration {
    fn from(uptime: Uptime) -> Self {
        Duration::from_secs(uptime.0)
    }
}

impl ToBytes for Uptime {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Uptime {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((Uptime(inner), remainder))
    }
}

/// Type representing changes in consensus validators.
#[derive(Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ConsensusValidatorChanges(pub BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>);

impl From<ConsensusValidatorChanges> for BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>> {
    fn from(consensus_validator_changes: ConsensusValidatorChanges) -> Self {
        consensus_validator_changes.0
    }
}

impl ToBytes for ConsensusValidatorChanges {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for ConsensusValidatorChanges {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((ConsensusValidatorChanges(inner), remainder))
    }
}

/// Type representing network name.
#[derive(Debug)]
pub struct NetworkName(pub String);

impl ToBytes for NetworkName {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl From<NetworkName> for String {
    fn from(network_name: NetworkName) -> Self {
        network_name.0
    }
}

impl FromBytes for NetworkName {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((NetworkName(inner), remainder))
    }
}

/// Type representing last progress of the sync process.
#[derive(Debug)]
pub struct LastProgress(pub Timestamp);

impl From<LastProgress> for Timestamp {
    fn from(last_progress: LastProgress) -> Self {
        last_progress.0
    }
}

impl ToBytes for LastProgress {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for LastProgress {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((LastProgress(inner), remainder))
    }
}

/// Type representing results of checking whether a block is in the highest sequence of available blocks.
#[derive(Debug)]
pub struct HighestBlockSequenceCheckResult(pub bool);

impl ToBytes for HighestBlockSequenceCheckResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for HighestBlockSequenceCheckResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((HighestBlockSequenceCheckResult(inner), remainder))
    }
}

/// Type representing results of the speculative execution.
#[derive(Debug)]
pub struct SpeculativeExecutionResult(pub Option<(ExecutionResultV2, Messages)>);

impl ToBytes for SpeculativeExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for SpeculativeExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((SpeculativeExecutionResult(inner), remainder))
    }
}
