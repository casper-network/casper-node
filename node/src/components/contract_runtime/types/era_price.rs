use crate::types::DataSize;
use casper_types::EraId;

#[derive(Clone, Copy, Ord, Eq, PartialOrd, PartialEq, DataSize, Debug)]
pub(crate) struct EraPrice {
    era_id: EraId,
    gas_price: u8,
}

impl EraPrice {
    pub(crate) fn new(era_id: EraId, gas_price: u8) -> Self {
        Self { era_id, gas_price }
    }

    pub(crate) fn gas_price(&self) -> u8 {
        self.gas_price
    }

    pub(crate) fn maybe_gas_price_for_era_id(&self, era_id: EraId) -> Option<u8> {
        if self.era_id == era_id {
            return Some(self.gas_price);
        }

        None
    }
}
