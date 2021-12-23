use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// The number of bytes in a serialized [`Weight`].
pub const WEIGHT_SERIALIZED_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// The weight attributed to a given [`AccountHash`](super::AccountHash) in an account's associated
/// keys.
#[derive(PartialOrd, Ord, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Weight(u8);

impl Weight {
    /// Constructs a new `Weight`.
    pub fn new(weight: u8) -> Weight {
        Weight(weight)
    }

    /// Returns the value of `self` as a `u8`.
    pub fn value(self) -> u8 {
        self.0
    }
}

impl ToBytes for Weight {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        WEIGHT_SERIALIZED_LENGTH
    }

    fn write_bytes<W>(&self, writer: &mut W) -> Result<(), bytesrepr::Error>
    where
        W: bytesrepr::Writer,
    {
        writer.write_u8(self.0)?;
        Ok(())
    }
}

impl FromBytes for Weight {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (byte, rem) = u8::from_bytes(bytes)?;
        Ok((Weight::new(byte), rem))
    }
}

impl CLTyped for Weight {
    fn cl_type() -> CLType {
        CLType::U8
    }
}
