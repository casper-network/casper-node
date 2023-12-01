//! Type aliases.

use alloc::collections::BTreeMap;
use datasize::DataSize;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, PublicKey, Timestamp, ValidatorChange,
};

#[derive(Debug)]
pub struct Uptime(pub u64);

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

#[derive(DataSize, Debug)]
pub struct ConsensusValidatorChanges(pub BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>);

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

impl FromBytes for NetworkName {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((NetworkName(inner), remainder))
    }
}

#[derive(Debug)]
pub struct LastProgress(pub Timestamp);

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
