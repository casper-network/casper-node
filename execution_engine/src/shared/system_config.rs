pub mod auction_costs;

use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::storage::protocol_data::DEFAULT_WASMLESS_TRANSFER_COST;

use self::auction_costs::AuctionCosts;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct SystemConfig {
    /// Wasmless transfer cost expressed in gas.
    wasmless_transfer_cost: u64,

    /// Configuration of auction entrypoint costs.
    auction_costs: AuctionCosts,
}

impl SystemConfig {
    pub fn new(wasmless_transfer_cost: u64, auction_costs: AuctionCosts) -> Self {
        Self {
            wasmless_transfer_cost,
            auction_costs,
        }
    }

    pub fn wasmless_transfer_cost(&self) -> u64 {
        self.wasmless_transfer_cost
    }

    pub fn auction_costs(&self) -> &AuctionCosts {
        &self.auction_costs
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            wasmless_transfer_cost: DEFAULT_WASMLESS_TRANSFER_COST,
            auction_costs: AuctionCosts::default(),
        }
    }
}

impl Distribution<SystemConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SystemConfig {
        SystemConfig {
            wasmless_transfer_cost: rng.gen(),
            auction_costs: rng.gen(),
        }
    }
}

impl ToBytes for SystemConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.wasmless_transfer_cost.to_bytes()?);
        ret.append(&mut self.auction_costs.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.wasmless_transfer_cost.serialized_length() + self.auction_costs.serialized_length()
    }
}

impl FromBytes for SystemConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (wasmless_transfer_cost, rem) = FromBytes::from_bytes(bytes)?;
        let (auction_costs, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            SystemConfig::new(wasmless_transfer_cost, auction_costs),
            rem,
        ))
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::{auction_costs::gens::auction_costs_arb, SystemConfig};

    prop_compose! {
        pub fn system_config_arb()(
            wasmless_transfer_cost in num::u64::ANY,
            auction_costs in auction_costs_arb(),
        ) -> SystemConfig {
            SystemConfig {
                wasmless_transfer_cost,
                auction_costs
            }
        }
    }
}
