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
use super::{NativeTransaction, UserlandTransaction};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    transaction::{AuctionTransaction, DirectCall},
    RuntimeArgs,
};

const NATIVE_TAG: u8 = 0;
const USERLAND_TAG: u8 = 1;

/// The high-level kind of a given [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The high-level kind of a given Transaction.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum TransactionKind {
    /// A `Transaction` targeting native functionality.
    Native(NativeTransaction),
    /// A `Transaction` with userland (i.e. not native) functionality.
    Userland(UserlandTransaction),
}

impl TransactionKind {
    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            TransactionKind::Native(native_transaction) => native_transaction.args(),
            TransactionKind::Userland(userland_transaction) => userland_transaction.args(),
        }
    }

    /// Returns a random `TransactionKind`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => TransactionKind::Native(NativeTransaction::random(rng)),
            1 => TransactionKind::Userland(UserlandTransaction::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl From<DirectCall> for TransactionKind {
    fn from(direct_call: DirectCall) -> Self {
        TransactionKind::Userland(UserlandTransaction::DirectCall(direct_call))
    }
}

impl From<AuctionTransaction> for TransactionKind {
    fn from(auction_transaction: AuctionTransaction) -> Self {
        TransactionKind::Native(NativeTransaction::Auction(auction_transaction))
    }
}

impl From<NativeTransaction> for TransactionKind {
    fn from(native_transaction: NativeTransaction) -> Self {
        TransactionKind::Native(native_transaction)
    }
}

impl From<UserlandTransaction> for TransactionKind {
    fn from(userland_transaction: UserlandTransaction) -> Self {
        TransactionKind::Userland(userland_transaction)
    }
}

impl Display for TransactionKind {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionKind::Native(txn) => write!(formatter, "kind: {}", txn),
            TransactionKind::Userland(txn) => write!(formatter, "kind: {}", txn),
        }
    }
}

impl ToBytes for TransactionKind {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionKind::Native(txn) => {
                NATIVE_TAG.write_bytes(writer)?;
                txn.write_bytes(writer)
            }
            TransactionKind::Userland(txn) => {
                USERLAND_TAG.write_bytes(writer)?;
                txn.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransactionKind::Native(txn) => txn.serialized_length(),
                TransactionKind::Userland(txn) => txn.serialized_length(),
            }
    }
}

impl FromBytes for TransactionKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            NATIVE_TAG => {
                let (txn, remainder) = NativeTransaction::from_bytes(remainder)?;
                Ok((TransactionKind::Native(txn), remainder))
            }
            USERLAND_TAG => {
                let (txn, remainder) = UserlandTransaction::from_bytes(remainder)?;
                Ok((TransactionKind::Userland(txn), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&TransactionKind::random(rng));
    }
}
