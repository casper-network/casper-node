use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::storage::protocol_data::DEFAULT_WASMLESS_TRANSFER_COST;

// #[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
// pub struct AuctionContractConfig {
//     get_era_validators: u32,
//     read_seigniorage_recipients: u32,
//     add_bid: u32,
//     withdraw_bid: u32,
//     delegate: u32,
//     undelegate: u32,
//     run_auction: u32,
//     slash: u32,
//     release_founder_stake: u32,
//     distribute: u32,
//     withdraw_delegator_reward: u32,
//     withdraw_validator_reward: u32,
//     read_era_id: u32,
// }

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct SystemConfig {
    // Wasmless transfer cost expressed in gas.
    wasmless_transfer_cost: u64,
    // auction_contract_config: AuctionContractConfig,
}

impl SystemConfig {
    pub fn new(wasmless_transfer_cost: u64) -> Self {
        Self {
            wasmless_transfer_cost,
        }
    }

    pub fn wasmless_transfer_cost(&self) -> u64 {
        self.wasmless_transfer_cost
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            wasmless_transfer_cost: DEFAULT_WASMLESS_TRANSFER_COST,
        }
    }
}

impl Distribution<SystemConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SystemConfig {
        SystemConfig {
            wasmless_transfer_cost: rng.gen(),
        }
    }
}

impl ToBytes for SystemConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.wasmless_transfer_cost.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.wasmless_transfer_cost.serialized_length()
    }
}

impl FromBytes for SystemConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (wasmless_transfer_cost, rem) = FromBytes::from_bytes(bytes)?;
        Ok((SystemConfig::new(wasmless_transfer_cost), rem))
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::SystemConfig;

    prop_compose! {
        pub fn system_config_arb()(
            wasmless_transfer_cost in num::u64::ANY,
        ) -> SystemConfig {
            SystemConfig {
                wasmless_transfer_cost,
            }
        }
    }
}
