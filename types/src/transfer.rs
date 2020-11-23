use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    DeployHash, URef, U512,
};

/// Represents a transfer from one purse to another
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub struct Transfer {
    /// Deploy that created the transfer
    pub deploy_hash: DeployHash,
    /// Account from which transfer was executed
    pub from: AccountHash,
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

impl Transfer {
    /// Creates a [`Transfer`].
    pub fn new(
        deploy_hash: DeployHash,
        from: AccountHash,
        source: URef,
        target: URef,
        amount: U512,
        gas: U512,
        id: Option<u64>,
    ) -> Self {
        Transfer {
            deploy_hash,
            from,
            source,
            target,
            amount,
            gas,
            id,
        }
    }
}

impl FromBytes for Transfer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (deploy_hash, rem) = DeployHash::from_bytes(bytes)?;
        let (from, rem) = AccountHash::from_bytes(rem)?;
        let (source, rem) = URef::from_bytes(rem)?;
        let (target, rem) = URef::from_bytes(rem)?;
        let (amount, rem) = U512::from_bytes(rem)?;
        let (gas, rem) = U512::from_bytes(rem)?;
        let (id, rem) = <Option<u64>>::from_bytes(rem)?;
        Ok((
            Transfer {
                deploy_hash,
                from,
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

impl ToBytes for Transfer {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.deploy_hash.to_bytes()?);
        result.append(&mut self.from.to_bytes()?);
        result.append(&mut self.source.to_bytes()?);
        result.append(&mut self.target.to_bytes()?);
        result.append(&mut self.amount.to_bytes()?);
        result.append(&mut self.gas.to_bytes()?);
        result.append(&mut self.id.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.deploy_hash.serialized_length()
            + self.from.serialized_length()
            + self.source.serialized_length()
            + self.target.serialized_length()
            + self.amount.serialized_length()
            + self.gas.serialized_length()
            + self.id.serialized_length()
    }
}

#[cfg(test)]
mod gens {
    use proptest::prelude::{prop::option, Arbitrary, Strategy};

    use crate::{
        deploy_info::gens::{account_hash_arb, deploy_hash_arb},
        gens::{u512_arb, uref_arb},
        Transfer,
    };

    pub fn transfer_arb() -> impl Strategy<Value = Transfer> {
        (
            deploy_hash_arb(),
            account_hash_arb(),
            uref_arb(),
            uref_arb(),
            u512_arb(),
            u512_arb(),
            option::of(<u64>::arbitrary()),
        )
            .prop_map(
                |(deploy_hash, from, source, target, amount, gas, id)| Transfer {
                    deploy_hash,
                    from,
                    source,
                    target,
                    amount,
                    gas,
                    id,
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
        fn test_serialization_roundtrip(transfer in gens::transfer_arb()) {
            bytesrepr::test_serialization_roundtrip(&transfer)
        }
    }
}
