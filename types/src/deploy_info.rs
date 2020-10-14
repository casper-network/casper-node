use alloc::vec::Vec;

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    key::TransferAddr,
    DeployHash, URef, U512,
};

/// Represents a transfer from one purse to another
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct DeployInfo {
    /// Deploy
    pub deploy_hash: DeployHash,
    /// Transfers
    pub transfers: Vec<TransferAddr>,
    /// Account
    pub from: AccountHash,
    /// Source purse
    pub source: URef,
    /// Gas
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
        result.append(&mut self.deploy_hash.to_bytes()?);
        result.append(&mut self.transfers.to_bytes()?);
        result.append(&mut self.from.to_bytes()?);
        result.append(&mut self.source.to_bytes()?);
        result.append(&mut self.gas.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.deploy_hash.serialized_length()
            + self.transfers.serialized_length()
            + self.from.serialized_length()
            + self.source.serialized_length()
            + self.gas.serialized_length()
    }
}
