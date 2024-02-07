use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// The session kind of a [`Transaction`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Session kind of a Transaction.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum TransactionSessionKind {
    /// A standard (non-special-case) session.
    ///
    /// This kind of session is not allowed to install or upgrade a stored contract, but can call
    /// stored contracts.
    Standard = 0,
    /// A session which installs a stored contract.
    Installer = 1,
    /// A session which upgrades a previously-installed stored contract.  Such a session must have
    /// "package_id: PackageIdentifier" runtime arg present.
    Upgrader = 2,
    /// A session which doesn't call any stored contracts.
    ///
    /// This kind of session is not allowed to install or upgrade a stored contract.
    Isolated = 3,
}

impl TransactionSessionKind {
    /// Returns a random `TransactionSessionKind`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..4) {
            v if v == TransactionSessionKind::Standard as u8 => TransactionSessionKind::Standard,
            v if v == TransactionSessionKind::Installer as u8 => TransactionSessionKind::Installer,
            v if v == TransactionSessionKind::Upgrader as u8 => TransactionSessionKind::Upgrader,
            v if v == TransactionSessionKind::Isolated as u8 => TransactionSessionKind::Isolated,
            _ => unreachable!(),
        }
    }
}

impl Display for TransactionSessionKind {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionSessionKind::Standard => write!(formatter, "standard"),
            TransactionSessionKind::Installer => write!(formatter, "installer"),
            TransactionSessionKind::Upgrader => write!(formatter, "upgrader"),
            TransactionSessionKind::Isolated => write!(formatter, "isolated"),
        }
    }
}

impl ToBytes for TransactionSessionKind {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        (*self as u8).write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for TransactionSessionKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            v if v == TransactionSessionKind::Standard as u8 => {
                Ok((TransactionSessionKind::Standard, remainder))
            }
            v if v == TransactionSessionKind::Installer as u8 => {
                Ok((TransactionSessionKind::Installer, remainder))
            }
            v if v == TransactionSessionKind::Upgrader as u8 => {
                Ok((TransactionSessionKind::Upgrader, remainder))
            }
            v if v == TransactionSessionKind::Isolated as u8 => {
                Ok((TransactionSessionKind::Isolated, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionSessionKind::random(rng));
        }
    }
}
