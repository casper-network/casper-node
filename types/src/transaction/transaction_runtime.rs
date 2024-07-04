use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::serialization::{
    tag_only_serialized_length,
    transaction_runtime::{deserialize_transaction_runtime, serialize_transaction_runtime},
};
#[cfg(doc)]
use super::Transaction;
use crate::bytesrepr::{self, FromBytes, ToBytes};

/// The runtime used to execute a [`Transaction`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Runtime used to execute a Transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum TransactionRuntime {
    /// The Casper Version 1 Virtual Machine.
    VmCasperV1,
    /// The Casper Version 2 Virtual Machine.
    VmCasperV2,
}

impl Display for TransactionRuntime {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionRuntime::VmCasperV1 => write!(formatter, "vm-casper-v1"),
            TransactionRuntime::VmCasperV2 => write!(formatter, "vm-casper-v2"),
        }
    }
}

impl ToBytes for TransactionRuntime {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        serialize_transaction_runtime(self)
    }

    fn serialized_length(&self) -> usize {
        tag_only_serialized_length()
    }
}

impl FromBytes for TransactionRuntime {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        deserialize_transaction_runtime(bytes)
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<TransactionRuntime> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> TransactionRuntime {
        match rng.gen_range(0..=1) {
            0 => TransactionRuntime::VmCasperV1,
            1 => TransactionRuntime::VmCasperV2,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        for transaction_runtime in [
            TransactionRuntime::VmCasperV1,
            TransactionRuntime::VmCasperV2,
        ] {
            bytesrepr::test_serialization_roundtrip(&transaction_runtime);
        }
    }
}
