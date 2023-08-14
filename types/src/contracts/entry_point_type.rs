#![allow(non_upper_case_globals)] // bitflags 2.x causes wasm bloat, so we need this attribute

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Context of method execution
///
/// Most significant bit represents version i.e.
/// - 0b0 -> 0.x/1.x (session & contracts)
/// - 0b1 -> 2.x and later (introduced installer, utility entry points)
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromPrimitive)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointType {
    /// Runs as session code
    Session = 0b00000000,
    /// Runs within contract's context
    Contract = 0b00000001,
    /// Pattern of entry point types introduced in 2.0.
    ///
    /// If this bit is missing, that means given entry point type was defined in pre-2.0 world.
    /// Installer entry point.
    Install = 0b10000000,
    /// Normal entry point.
    Normal = 0b10000001,
}

impl EntryPointType {
    /// Checks if entry point type is introduced before 2.0.
    pub fn is_legacy_pattern(&self) -> bool {
        (*self as u8) & 0b10000000 == 0
    }

    /// Get the bit pattern.
    pub fn bits(self) -> u8 {
        self as u8
    }
}

impl ToBytes for EntryPointType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.bits().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        1
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.bits());
        Ok(())
    }
}

impl FromBytes for EntryPointType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, bytes) = u8::from_bytes(bytes)?;
        let entry_point_type =
            EntryPointType::from_u8(value).ok_or(bytesrepr::Error::Formatting)?;
        Ok((entry_point_type, bytes))
    }
}
