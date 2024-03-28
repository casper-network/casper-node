mod transfer_v1_addr;

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    serde_helpers, DeployHash, URef, U512,
};
pub use transfer_v1_addr::{TransferAddr, TRANSFER_ADDR_LENGTH};

/// Represents a version 1 transfer from one purse to another.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransferV1 {
    /// Deploy that created the transfer
    #[serde(with = "serde_helpers::deploy_hash_as_array")]
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            with = "DeployHash",
            description = "Hex-encoded Deploy hash of Deploy that created the transfer."
        )
    )]
    pub deploy_hash: DeployHash,
    /// Account from which transfer was executed
    pub from: AccountHash,
    /// Account to which funds are transferred
    pub to: Option<AccountHash>,
    /// Source purse
    pub source: URef,
    /// Target purse
    pub target: URef,
    /// Transfer amount
    pub amount: U512,
    /// Gas
    pub gas: U512,
    /// User-defined id
    pub id: Option<u64>,
}

impl TransferV1 {
    /// Creates a [`TransferV1`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        deploy_hash: DeployHash,
        from: AccountHash,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        gas: U512,
        id: Option<u64>,
    ) -> Self {
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
    }
}

impl ToBytes for TransferV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.deploy_hash.serialized_length()
            + self.from.serialized_length()
            + self.to.serialized_length()
            + self.source.serialized_length()
            + self.target.serialized_length()
            + self.amount.serialized_length()
            + self.gas.serialized_length()
            + self.id.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.deploy_hash.write_bytes(writer)?;
        self.from.write_bytes(writer)?;
        self.to.write_bytes(writer)?;
        self.source.write_bytes(writer)?;
        self.target.write_bytes(writer)?;
        self.amount.write_bytes(writer)?;
        self.gas.write_bytes(writer)?;
        self.id.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for TransferV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (deploy_hash, rem) = FromBytes::from_bytes(bytes)?;
        let (from, rem) = AccountHash::from_bytes(rem)?;
        let (to, rem) = <Option<AccountHash>>::from_bytes(rem)?;
        let (source, rem) = URef::from_bytes(rem)?;
        let (target, rem) = URef::from_bytes(rem)?;
        let (amount, rem) = U512::from_bytes(rem)?;
        let (gas, rem) = U512::from_bytes(rem)?;
        let (id, rem) = <Option<u64>>::from_bytes(rem)?;
        Ok((
            TransferV1 {
                deploy_hash,
                from,
                to,
                source,
                target,
                amount,
                gas,
                id,
            },
            rem,
        ))
    }
}
