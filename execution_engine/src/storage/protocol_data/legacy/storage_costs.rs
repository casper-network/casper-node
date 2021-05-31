use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::storage_costs::StorageCosts;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LegacyStorageCosts(StorageCosts);

impl From<LegacyStorageCosts> for StorageCosts {
    fn from(legacy_storage_costs: LegacyStorageCosts) -> Self {
        legacy_storage_costs.0
    }
}

impl FromBytes for LegacyStorageCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (gas_per_byte, rem) = u32::from_bytes(bytes)?;

        let storage_costs = StorageCosts::new(gas_per_byte);

        Ok((LegacyStorageCosts(storage_costs), rem))
    }
}
