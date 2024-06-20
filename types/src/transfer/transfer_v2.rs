use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    transaction::TransactionHash,
    Gas, InitiatorAddr, URef, U512,
};

/// Represents a version 2 transfer from one purse to another.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransferV2 {
    /// Transaction that created the transfer.
    pub transaction_hash: TransactionHash,
    /// Entity from which transfer was executed.
    pub from: InitiatorAddr,
    /// Account to which funds are transferred.
    pub to: Option<AccountHash>,
    /// Source purse.
    pub source: URef,
    /// Target purse.
    pub target: URef,
    /// Transfer amount.
    pub amount: U512,
    /// Gas.
    pub gas: Gas,
    /// User-defined ID.
    pub id: Option<u64>,
}

impl TransferV2 {
    /// Creates a [`TransferV2`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transaction_hash: TransactionHash,
        from: InitiatorAddr,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        gas: Gas,
        id: Option<u64>,
    ) -> Self {
        TransferV2 {
            transaction_hash,
            from,
            to,
            source,
            target,
            amount,
            gas,
            id,
        }
    }
}

impl ToBytes for TransferV2 {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buf = Vec::new();
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    fn serialized_length(&self) -> usize {
        self.transaction_hash.serialized_length()
            + self.from.serialized_length()
            + self.to.serialized_length()
            + self.source.serialized_length()
            + self.target.serialized_length()
            + self.amount.serialized_length()
            + self.gas.serialized_length()
            + self.id.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transaction_hash.write_bytes(writer)?;
        self.from.write_bytes(writer)?;
        self.to.write_bytes(writer)?;
        self.source.write_bytes(writer)?;
        self.target.write_bytes(writer)?;
        self.amount.write_bytes(writer)?;
        self.gas.write_bytes(writer)?;
        self.id.write_bytes(writer)
    }
}

impl FromBytes for TransferV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transaction_hash, remainder) = TransactionHash::from_bytes(bytes)?;
        let (from, remainder) = InitiatorAddr::from_bytes(remainder)?;
        let (to, remainder) = <Option<AccountHash>>::from_bytes(remainder)?;
        let (source, remainder) = URef::from_bytes(remainder)?;
        let (target, remainder) = URef::from_bytes(remainder)?;
        let (amount, remainder) = U512::from_bytes(remainder)?;
        let (gas, remainder) = Gas::from_bytes(remainder)?;
        let (id, remainder) = <Option<u64>>::from_bytes(remainder)?;
        Ok((
            TransferV2 {
                transaction_hash,
                from,
                to,
                source,
                target,
                amount,
                gas,
                id,
            },
            remainder,
        ))
    }
}
