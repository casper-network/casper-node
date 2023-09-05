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
use super::TransactionV1;
use super::{NativeTransactionV1, UserlandTransactionV1};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    transaction::{AuctionTransactionV1, DirectCallV1},
    RuntimeArgs,
};

const NATIVE_TAG: u8 = 0;
const USERLAND_TAG: u8 = 1;

/// The high-level kind of a given [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The high-level kind of a given TransactionV1.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum TransactionV1Kind {
    /// A transaction targeting native functionality.
    Native(NativeTransactionV1),
    /// A transaction with userland (i.e. not native) functionality.
    Userland(UserlandTransactionV1),
}

impl TransactionV1Kind {
    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            TransactionV1Kind::Native(native_transaction) => native_transaction.args(),
            TransactionV1Kind::Userland(userland_transaction) => userland_transaction.args(),
        }
    }

    /// Returns a random `TransactionV1Kind`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => TransactionV1Kind::Native(NativeTransactionV1::random(rng)),
            1 => TransactionV1Kind::Userland(UserlandTransactionV1::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl From<DirectCallV1> for TransactionV1Kind {
    fn from(direct_call: DirectCallV1) -> Self {
        TransactionV1Kind::Userland(UserlandTransactionV1::DirectCall(direct_call))
    }
}

impl From<AuctionTransactionV1> for TransactionV1Kind {
    fn from(auction_transaction: AuctionTransactionV1) -> Self {
        TransactionV1Kind::Native(NativeTransactionV1::Auction(auction_transaction))
    }
}

impl From<NativeTransactionV1> for TransactionV1Kind {
    fn from(native_transaction: NativeTransactionV1) -> Self {
        TransactionV1Kind::Native(native_transaction)
    }
}

impl From<UserlandTransactionV1> for TransactionV1Kind {
    fn from(userland_transaction: UserlandTransactionV1) -> Self {
        TransactionV1Kind::Userland(userland_transaction)
    }
}

impl Display for TransactionV1Kind {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionV1Kind::Native(txn) => write!(formatter, "kind: {}", txn),
            TransactionV1Kind::Userland(txn) => write!(formatter, "kind: {}", txn),
        }
    }
}

impl ToBytes for TransactionV1Kind {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionV1Kind::Native(txn) => {
                NATIVE_TAG.write_bytes(writer)?;
                txn.write_bytes(writer)
            }
            TransactionV1Kind::Userland(txn) => {
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
                TransactionV1Kind::Native(txn) => txn.serialized_length(),
                TransactionV1Kind::Userland(txn) => txn.serialized_length(),
            }
    }
}

impl FromBytes for TransactionV1Kind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            NATIVE_TAG => {
                let (txn, remainder) = NativeTransactionV1::from_bytes(remainder)?;
                Ok((TransactionV1Kind::Native(txn), remainder))
            }
            USERLAND_TAG => {
                let (txn, remainder) = UserlandTransactionV1::from_bytes(remainder)?;
                Ok((TransactionV1Kind::Userland(txn), remainder))
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
        bytesrepr::test_serialization_roundtrip(&TransactionV1Kind::random(rng));
    }
}
