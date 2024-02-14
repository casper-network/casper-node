use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Gas, InitiatorAddr, TransactionHash, TransferAddr, URef,
};

/// Information relating to the given Transaction, stored in global state under .
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransactionInfo {
    /// The transaction hash.
    pub transaction_hash: TransactionHash,
    /// Transfers done during execution of the transaction.
    pub transfers: Vec<TransferAddr>,
    /// Identifier of the initiator of the transaction.
    pub from: InitiatorAddr,
    /// Source purse used for payment of the transaction.
    pub source: URef,
    /// Gas used in executing the transaction.
    pub gas: Gas,
}

impl TransactionInfo {
    /// Returns a new `TransactionInfo`.
    pub fn new(
        transaction_hash: TransactionHash,
        transfers: Vec<TransferAddr>,
        from: InitiatorAddr,
        source: URef,
        gas: Gas,
    ) -> Self {
        TransactionInfo {
            transaction_hash,
            transfers,
            from,
            source,
            gas,
        }
    }
}

impl ToBytes for TransactionInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buf = Vec::new();
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    fn serialized_length(&self) -> usize {
        self.transaction_hash.serialized_length()
            + self.transfers.serialized_length()
            + self.from.serialized_length()
            + self.source.serialized_length()
            + self.gas.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transaction_hash.write_bytes(writer)?;
        self.transfers.write_bytes(writer)?;
        self.from.write_bytes(writer)?;
        self.source.write_bytes(writer)?;
        self.gas.write_bytes(writer)
    }
}

impl FromBytes for TransactionInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transaction_hash, remainder) = TransactionHash::from_bytes(bytes)?;
        let (transfers, remainder) = Vec::<TransferAddr>::from_bytes(remainder)?;
        let (from, remainder) = InitiatorAddr::from_bytes(remainder)?;
        let (source, remainder) = URef::from_bytes(remainder)?;
        let (gas, remainder) = Gas::from_bytes(remainder)?;
        Ok((
            TransactionInfo {
                transaction_hash,
                transfers,
                from,
                source,
                gas,
            },
            remainder,
        ))
    }
}

/// Generators for a `TransactionInfo`.
#[cfg(any(feature = "testing", feature = "gens", test))]
pub(crate) mod gens {
    use alloc::vec::Vec;

    use proptest::{
        array,
        collection::{self, SizeRange},
        prelude::{Arbitrary, Strategy},
        prop_oneof,
    };

    use crate::{
        account::AccountHash,
        crypto::gens::public_key_arb_no_system,
        gens::{u512_arb, uref_arb},
        DeployHash, Gas, InitiatorAddr, TransactionHash, TransactionInfo, TransactionV1Hash,
        TransferAddr,
    };

    pub fn deploy_hash_arb() -> impl Strategy<Value = DeployHash> {
        array::uniform32(<u8>::arbitrary()).prop_map(DeployHash::from_raw)
    }

    pub fn txn_v1_hash_arb() -> impl Strategy<Value = TransactionV1Hash> {
        array::uniform32(<u8>::arbitrary()).prop_map(TransactionV1Hash::from_raw)
    }

    pub fn txn_hash_arb() -> impl Strategy<Value = TransactionHash> {
        prop_oneof![
            deploy_hash_arb().prop_map(TransactionHash::from),
            txn_v1_hash_arb().prop_map(TransactionHash::from)
        ]
    }

    pub fn transfer_addr_arb() -> impl Strategy<Value = TransferAddr> {
        array::uniform32(<u8>::arbitrary()).prop_map(TransferAddr::new)
    }

    pub fn transfers_arb(size: impl Into<SizeRange>) -> impl Strategy<Value = Vec<TransferAddr>> {
        collection::vec(transfer_addr_arb(), size)
    }

    pub fn account_hash_arb() -> impl Strategy<Value = AccountHash> {
        array::uniform32(<u8>::arbitrary()).prop_map(AccountHash::new)
    }

    pub fn initiator_addr_arb() -> impl Strategy<Value = InitiatorAddr> {
        prop_oneof![
            public_key_arb_no_system().prop_map(InitiatorAddr::from),
            account_hash_arb().prop_map(InitiatorAddr::from),
        ]
    }

    /// Arbitrary `TransactionInfo`.
    pub fn txn_info_arb() -> impl Strategy<Value = TransactionInfo> {
        let transfers_length_range = 0..5;
        (
            txn_hash_arb(),
            transfers_arb(transfers_length_range),
            initiator_addr_arb(),
            uref_arb(),
            u512_arb(),
        )
            .prop_map(
                |(transaction_hash, transfers, from, source, gas)| TransactionInfo {
                    transaction_hash,
                    transfers,
                    from,
                    source,
                    gas: Gas::new(gas),
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::bytesrepr;

    use super::gens;

    proptest! {
        #[test]
        fn test_serialization_roundtrip(txn_info in gens::txn_info_arb()) {
            bytesrepr::test_serialization_roundtrip(&txn_info)
        }
    }
}
