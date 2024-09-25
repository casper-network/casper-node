mod error;
mod transfer_v1;
mod transfer_v2;

use alloc::vec::Vec;

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(feature = "json-schema")]
use crate::{account::AccountHash, transaction::TransactionV1Hash, URef, U512};
#[cfg(any(feature = "testing", feature = "json-schema", test))]
use crate::{transaction::TransactionHash, Gas, InitiatorAddr};
pub use error::TransferFromStrError;
pub use transfer_v1::{TransferAddr, TransferV1, TRANSFER_ADDR_LENGTH};
pub use transfer_v2::TransferV2;

const V1_TAG: u8 = 0;
const V2_TAG: u8 = 1;

#[cfg(feature = "json-schema")]
pub(super) static TRANSFER: Lazy<Transfer> = Lazy::new(|| {
    let transaction_hash = TransactionHash::V1(TransactionV1Hash::from_raw([1; 32]));
    let from = InitiatorAddr::AccountHash(AccountHash::new([2; 32]));
    let to = Some(AccountHash::new([3; 32]));
    let source = URef::from_formatted_str(
        "uref-0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a-007",
    )
    .unwrap();
    let target = URef::from_formatted_str(
        "uref-1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b-000",
    )
    .unwrap();
    let amount = U512::from(1_000_000_000_000_u64);
    let gas = Gas::new(2_500_000_000_u64);
    let id = Some(999);
    Transfer::V2(TransferV2::new(
        transaction_hash,
        from,
        to,
        source,
        target,
        amount,
        gas,
        id,
    ))
});

/// A versioned wrapper for a transfer.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum Transfer {
    /// A version 1 transfer.
    #[serde(rename = "Version1")]
    V1(TransferV1),
    /// A version 2 transfer.
    #[serde(rename = "Version2")]
    V2(TransferV2),
}

impl Transfer {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &TRANSFER
    }

    /// Returns a random `Transfer`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use crate::DeployHash;

        if rng.gen() {
            Transfer::V1(TransferV1::new(
                DeployHash::random(rng),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
            ))
        } else {
            Transfer::V2(TransferV2::new(
                TransactionHash::random(rng),
                InitiatorAddr::random(rng),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                Gas::new(rng.gen::<u64>()),
                rng.gen(),
            ))
        }
    }
}

impl From<TransferV1> for Transfer {
    fn from(v1_transfer: TransferV1) -> Self {
        Transfer::V1(v1_transfer)
    }
}

impl From<TransferV2> for Transfer {
    fn from(v2_transfer: TransferV2) -> Self {
        Transfer::V2(v2_transfer)
    }
}

impl ToBytes for Transfer {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            Transfer::V1(transfer) => {
                V1_TAG.write_bytes(writer)?;
                transfer.write_bytes(writer)
            }
            Transfer::V2(transfer) => {
                V2_TAG.write_bytes(writer)?;
                transfer.write_bytes(writer)
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
                Transfer::V1(transfer) => transfer.serialized_length(),
                Transfer::V2(transfer) => transfer.serialized_length(),
            }
    }
}

impl FromBytes for Transfer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            V1_TAG => {
                let (transfer, remainder) = TransferV1::from_bytes(remainder)?;
                Ok((Transfer::V1(transfer), remainder))
            }
            V2_TAG => {
                let (transfer, remainder) = TransferV2::from_bytes(remainder)?;
                Ok((Transfer::V2(transfer), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Proptest generators for [`Transfer`].
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens {
    use proptest::{
        array,
        prelude::{prop::option, Arbitrary, Strategy},
    };

    use super::*;
    use crate::{
        gens::{account_hash_arb, u512_arb, uref_arb},
        transaction::gens::deploy_hash_arb,
    };

    pub fn transfer_v1_addr_arb() -> impl Strategy<Value = TransferAddr> {
        array::uniform32(<u8>::arbitrary()).prop_map(TransferAddr::new)
    }

    pub fn transfer_v1_arb() -> impl Strategy<Value = TransferV1> {
        (
            deploy_hash_arb(),
            account_hash_arb(),
            option::of(account_hash_arb()),
            uref_arb(),
            uref_arb(),
            u512_arb(),
            u512_arb(),
            option::of(<u64>::arbitrary()),
        )
            .prop_map(|(deploy_hash, from, to, source, target, amount, gas, id)| {
                TransferV1 {
                    deploy_hash,
                    from,
                    to,
                    source,
                    target,
                    amount,
                    gas,
                    id,
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::bytesrepr;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let transfer = Transfer::random(rng);
        bytesrepr::test_serialization_roundtrip(&transfer);
    }
}
