// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    DeployHash, TransferAddr, URef, U512,
};

/// Information relating to the given Deploy.
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeployInfo {
    /// The relevant Deploy.
    pub deploy_hash: DeployHash,
    /// Transfers performed by the Deploy.
    pub transfers: Vec<TransferAddr>,
    /// Account identifier of the creator of the Deploy.
    pub from: AccountHash,
    /// Source purse used for payment of the Deploy.
    pub source: URef,
    /// Gas cost of executing the Deploy.
    pub gas: U512,
}

impl DeployInfo {
    /// Creates a [`DeployInfo`].
    pub fn new(
        deploy_hash: DeployHash,
        transfers: &[TransferAddr],
        from: AccountHash,
        source: URef,
        gas: U512,
    ) -> Self {
        let transfers = transfers.to_vec();
        DeployInfo {
            deploy_hash,
            transfers,
            from,
            source,
            gas,
        }
    }
}

impl FromBytes for DeployInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (deploy_hash, rem) = DeployHash::from_bytes(bytes)?;
        let (transfers, rem) = Vec::<TransferAddr>::from_bytes(rem)?;
        let (from, rem) = AccountHash::from_bytes(rem)?;
        let (source, rem) = URef::from_bytes(rem)?;
        let (gas, rem) = U512::from_bytes(rem)?;
        Ok((
            DeployInfo {
                deploy_hash,
                transfers,
                from,
                source,
                gas,
            },
            rem,
        ))
    }
}

impl ToBytes for DeployInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.deploy_hash.write_bytes(&mut result)?;
        self.transfers.write_bytes(&mut result)?;
        self.from.write_bytes(&mut result)?;
        self.source.write_bytes(&mut result)?;
        self.gas.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.deploy_hash.serialized_length()
            + self.transfers.serialized_length()
            + self.from.serialized_length()
            + self.source.serialized_length()
            + self.gas.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.deploy_hash.write_bytes(writer)?;
        self.transfers.write_bytes(writer)?;
        self.from.write_bytes(writer)?;
        self.source.write_bytes(writer)?;
        self.gas.write_bytes(writer)?;
        Ok(())
    }
}

/// Generators for a `Deploy`
#[cfg(any(feature = "testing", feature = "gens", test))]
pub(crate) mod gens {
    use alloc::vec::Vec;

    use proptest::{
        array,
        collection::{self, SizeRange},
        prelude::{Arbitrary, Strategy},
    };

    use crate::{
        account::AccountHash,
        gens::{u512_arb, uref_arb},
        DeployHash, DeployInfo, TransferAddr,
    };

    pub fn deploy_hash_arb() -> impl Strategy<Value = DeployHash> {
        array::uniform32(<u8>::arbitrary()).prop_map(DeployHash::new)
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

    /// Creates an arbitrary `Deploy`
    pub fn deploy_info_arb() -> impl Strategy<Value = DeployInfo> {
        let transfers_length_range = 0..5;
        (
            deploy_hash_arb(),
            transfers_arb(transfers_length_range),
            account_hash_arb(),
            uref_arb(),
            u512_arb(),
        )
            .prop_map(|(deploy_hash, transfers, from, source, gas)| DeployInfo {
                deploy_hash,
                transfers,
                from,
                source,
                gas,
            })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::bytesrepr;

    use super::gens;

    proptest! {
        #[test]
        fn test_serialization_roundtrip(deploy_info in gens::deploy_info_arb()) {
            bytesrepr::test_serialization_roundtrip(&deploy_info)
        }
    }
}
