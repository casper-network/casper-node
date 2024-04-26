#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use super::{AuctionCosts, EntityCosts, HandlePaymentCosts, MintCosts, StandardPaymentCosts};
use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Default gas limit of install / upgrade contracts
pub const DEFAULT_INSTALL_UPGRADE_GAS_LIMIT: u64 = 3_500_000_000_000;

/// Default gas limit of standard transactions
pub const DEFAULT_STANDARD_TRANSACTION_GAS_LIMIT: u64 = 500_000_000_000;

/// Definition of costs in the system.
///
/// This structure contains the costs of all the system contract's entry points and, additionally,
/// it defines a wasmless mint cost.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    /// Standard transaction gas limit expressed in gas.
    standard_transaction_gas_limit: u64,

    /// Install or upgrade transaction gas limit expressed in gas.
    install_upgrade_gas_limit: u64,

    /// Configuration of auction entrypoint costs.
    auction_costs: AuctionCosts,

    /// Configuration of mint entrypoint costs.
    mint_costs: MintCosts,

    /// Configuration of handle payment entrypoint costs.
    handle_payment_costs: HandlePaymentCosts,

    /// Configuration of standard payment costs.
    standard_payment_costs: StandardPaymentCosts,

    /// Configuration of entity entrypoint costs.
    entity_costs: EntityCosts,
}

impl SystemConfig {
    /// Creates new system config instance.
    pub fn new(
        install_upgrade_gas_limit: u64,
        standard_transaction_gas_limit: u64,
        auction_costs: AuctionCosts,
        mint_costs: MintCosts,
        handle_payment_costs: HandlePaymentCosts,
        standard_payment_costs: StandardPaymentCosts,
        entity_costs: EntityCosts,
    ) -> Self {
        Self {
            install_upgrade_gas_limit,
            standard_transaction_gas_limit,
            auction_costs,
            mint_costs,
            handle_payment_costs,
            standard_payment_costs,
            entity_costs,
        }
    }

    /// Returns install / upgrade cost.
    pub fn install_upgrade_limit(&self) -> u64 {
        self.install_upgrade_gas_limit
    }

    /// Returns standard / flat cost.
    pub fn standard_transaction_limit(&self) -> u64 {
        self.standard_transaction_gas_limit
    }

    /// Returns the costs of executing auction entry points.
    pub fn auction_costs(&self) -> &AuctionCosts {
        &self.auction_costs
    }

    /// Returns the costs of executing mint entry points.
    pub fn mint_costs(&self) -> &MintCosts {
        &self.mint_costs
    }

    /// Sets mint costs.
    pub fn with_mint_costs(mut self, mint_costs: MintCosts) -> Self {
        self.mint_costs = mint_costs;
        self
    }

    /// Returns the costs of executing `handle_payment` entry points.
    pub fn handle_payment_costs(&self) -> &HandlePaymentCosts {
        &self.handle_payment_costs
    }

    /// Returns the costs of executing `standard_payment` entry points.
    pub fn standard_payment_costs(&self) -> &StandardPaymentCosts {
        &self.standard_payment_costs
    }

    /// Returns the costs of executing `handle_payment` entry points.
    pub fn entity_costs(&self) -> &EntityCosts {
        &self.entity_costs
    }
}

#[cfg(any(feature = "testing", test))]
impl SystemConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        // there's a bug in toml...under the hood it uses an i64 when it should use a u64
        // this causes flaky test failures if the random result exceeds i64::MAX
        let install_upgrade_gas_limit = rng.gen::<u32>() as u64;
        let standard_transaction_gas_limit = rng.gen::<u32>() as u64;
        let auction_costs = rng.gen();
        let mint_costs = rng.gen();
        let handle_payment_costs = rng.gen();
        let standard_payment_costs = rng.gen();
        let entity_costs = rng.gen();

        SystemConfig {
            install_upgrade_gas_limit,
            standard_transaction_gas_limit,
            auction_costs,
            mint_costs,
            handle_payment_costs,
            standard_payment_costs,
            entity_costs,
        }
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            install_upgrade_gas_limit: DEFAULT_INSTALL_UPGRADE_GAS_LIMIT,
            standard_transaction_gas_limit: DEFAULT_STANDARD_TRANSACTION_GAS_LIMIT,
            auction_costs: AuctionCosts::default(),
            mint_costs: MintCosts::default(),
            handle_payment_costs: HandlePaymentCosts::default(),
            standard_payment_costs: StandardPaymentCosts::default(),
            entity_costs: EntityCosts::default(),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<SystemConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SystemConfig {
        SystemConfig {
            install_upgrade_gas_limit: rng.gen(),
            standard_transaction_gas_limit: rng.gen(),
            auction_costs: rng.gen(),
            mint_costs: rng.gen(),
            handle_payment_costs: rng.gen(),
            standard_payment_costs: rng.gen(),
            entity_costs: rng.gen(),
        }
    }
}

impl ToBytes for SystemConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.install_upgrade_gas_limit.to_bytes()?);
        ret.append(&mut self.standard_transaction_gas_limit.to_bytes()?);
        ret.append(&mut self.auction_costs.to_bytes()?);
        ret.append(&mut self.mint_costs.to_bytes()?);
        ret.append(&mut self.handle_payment_costs.to_bytes()?);
        ret.append(&mut self.standard_payment_costs.to_bytes()?);
        ret.append(&mut self.entity_costs.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.install_upgrade_gas_limit.serialized_length()
            + self.standard_transaction_gas_limit.serialized_length()
            + self.auction_costs.serialized_length()
            + self.mint_costs.serialized_length()
            + self.handle_payment_costs.serialized_length()
            + self.standard_payment_costs.serialized_length()
            + self.entity_costs.serialized_length()
    }
}

impl FromBytes for SystemConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (install_upgrade_cost, rem) = FromBytes::from_bytes(bytes)?;
        let (standard_transaction_cost, rem) = FromBytes::from_bytes(rem)?;
        let (auction_costs, rem) = FromBytes::from_bytes(rem)?;
        let (mint_costs, rem) = FromBytes::from_bytes(rem)?;
        let (handle_payment_costs, rem) = FromBytes::from_bytes(rem)?;
        let (standard_payment_costs, rem) = FromBytes::from_bytes(rem)?;
        let (entity_costs, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            SystemConfig::new(
                install_upgrade_cost,
                standard_transaction_cost,
                auction_costs,
                mint_costs,
                handle_payment_costs,
                standard_payment_costs,
                entity_costs,
            ),
            rem,
        ))
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use crate::{
        chainspec::vm_config::{
            auction_costs::gens::auction_costs_arb, entity_costs::gens::entity_costs_arb,
            handle_payment_costs::gens::handle_payment_costs_arb, mint_costs::gens::mint_costs_arb,
            standard_payment_costs::gens::standard_payment_costs_arb,
        },
        SystemConfig,
    };

    prop_compose! {
        pub fn system_config_arb()(
            install_upgrade_gas_limit in num::u32::ANY,
            standard_transaction_gas_limit in num::u32::ANY,
            auction_costs in auction_costs_arb(),
            mint_costs in mint_costs_arb(),
            handle_payment_costs in handle_payment_costs_arb(),
            standard_payment_costs in standard_payment_costs_arb(),
            entity_costs in entity_costs_arb(),
        ) -> SystemConfig {
            let install_upgrade_gas_limit = install_upgrade_gas_limit as u64;
            let standard_transaction_gas_limit =standard_transaction_gas_limit as u64;
            SystemConfig {
                install_upgrade_gas_limit,
                standard_transaction_gas_limit,
                auction_costs,
                mint_costs,
                handle_payment_costs,
                standard_payment_costs,
                entity_costs,
            }
        }
    }
}
