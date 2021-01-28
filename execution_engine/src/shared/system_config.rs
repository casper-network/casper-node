pub mod auction_costs;
pub mod mint_costs;
pub mod proof_of_stake_costs;
pub mod standard_payment_costs;

use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use self::{
    auction_costs::AuctionCosts, mint_costs::MintCosts, proof_of_stake_costs::ProofOfStakeCosts,
    standard_payment_costs::StandardPaymentCosts,
};
use crate::storage::protocol_data::DEFAULT_WASMLESS_TRANSFER_COST;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct SystemConfig {
    /// Wasmless transfer cost expressed in gas.
    wasmless_transfer_cost: u32,

    /// Configuration of auction entrypoint costs.
    auction_costs: AuctionCosts,

    /// Configuration of mint entrypoint costs.
    mint_costs: MintCosts,

    /// Configuration of proof of stake entrypoint costs.
    proof_of_stake_costs: ProofOfStakeCosts,

    /// Configuration of standard payment costs.
    standard_payment_costs: StandardPaymentCosts,
}

impl SystemConfig {
    pub fn new(
        wasmless_transfer_cost: u32,
        auction_costs: AuctionCosts,
        mint_costs: MintCosts,
        proof_of_stake_costs: ProofOfStakeCosts,
        standard_payment_costs: StandardPaymentCosts,
    ) -> Self {
        Self {
            wasmless_transfer_cost,
            auction_costs,
            mint_costs,
            proof_of_stake_costs,
            standard_payment_costs,
        }
    }

    pub fn wasmless_transfer_cost(&self) -> u32 {
        self.wasmless_transfer_cost
    }

    pub fn auction_costs(&self) -> &AuctionCosts {
        &self.auction_costs
    }

    pub fn mint_costs(&self) -> &MintCosts {
        &self.mint_costs
    }

    pub fn proof_of_stake_costs(&self) -> &ProofOfStakeCosts {
        &self.proof_of_stake_costs
    }

    pub fn standard_payment_costs(&self) -> &StandardPaymentCosts {
        &self.standard_payment_costs
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            wasmless_transfer_cost: DEFAULT_WASMLESS_TRANSFER_COST,
            auction_costs: AuctionCosts::default(),
            mint_costs: MintCosts::default(),
            proof_of_stake_costs: ProofOfStakeCosts::default(),
            standard_payment_costs: StandardPaymentCosts::default(),
        }
    }
}

impl Distribution<SystemConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SystemConfig {
        SystemConfig {
            wasmless_transfer_cost: rng.gen(),
            auction_costs: rng.gen(),
            mint_costs: rng.gen(),
            proof_of_stake_costs: rng.gen(),
            standard_payment_costs: rng.gen(),
        }
    }
}

impl ToBytes for SystemConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.wasmless_transfer_cost.to_bytes()?);
        ret.append(&mut self.auction_costs.to_bytes()?);
        ret.append(&mut self.mint_costs.to_bytes()?);
        ret.append(&mut self.proof_of_stake_costs.to_bytes()?);
        ret.append(&mut self.standard_payment_costs.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.wasmless_transfer_cost.serialized_length()
            + self.auction_costs.serialized_length()
            + self.mint_costs.serialized_length()
            + self.proof_of_stake_costs.serialized_length()
            + self.standard_payment_costs.serialized_length()
    }
}

impl FromBytes for SystemConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (wasmless_transfer_cost, rem) = FromBytes::from_bytes(bytes)?;
        let (auction_costs, rem) = FromBytes::from_bytes(rem)?;
        let (mint_costs, rem) = FromBytes::from_bytes(rem)?;
        let (proof_of_stake_costs, rem) = FromBytes::from_bytes(rem)?;
        let (standard_payment_costs, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            SystemConfig::new(
                wasmless_transfer_cost,
                auction_costs,
                mint_costs,
                proof_of_stake_costs,
                standard_payment_costs,
            ),
            rem,
        ))
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::{
        auction_costs::gens::auction_costs_arb, mint_costs::gens::mint_costs_arb,
        proof_of_stake_costs::gens::proof_of_stake_costs_arb,
        standard_payment_costs::gens::standard_payment_costs_arb, SystemConfig,
    };

    prop_compose! {
        pub fn system_config_arb()(
            wasmless_transfer_cost in num::u32::ANY,
            auction_costs in auction_costs_arb(),
            mint_costs in mint_costs_arb(),
            proof_of_stake_costs in proof_of_stake_costs_arb(),
            standard_payment_costs in standard_payment_costs_arb(),
        ) -> SystemConfig {
            SystemConfig {
                wasmless_transfer_cost,
                auction_costs,
                mint_costs,
                proof_of_stake_costs,
                standard_payment_costs,
            }
        }
    }
}
