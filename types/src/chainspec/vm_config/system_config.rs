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

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    chainspec::vm_config::{AuctionCosts, HandlePaymentCosts, MintCosts, StandardPaymentCosts},
};

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
    /// Configuration of auction entrypoint costs.
    auction_costs: AuctionCosts,

    /// Configuration of mint entrypoint costs.
    mint_costs: MintCosts,

    /// Configuration of handle payment entrypoint costs.
    handle_payment_costs: HandlePaymentCosts,

    /// Configuration of standard payment costs.
    standard_payment_costs: StandardPaymentCosts,
}

impl SystemConfig {
    /// Creates new system config instance.
    pub fn new(
        auction_costs: AuctionCosts,
        mint_costs: MintCosts,
        handle_payment_costs: HandlePaymentCosts,
        standard_payment_costs: StandardPaymentCosts,
    ) -> Self {
        Self {
            auction_costs,
            mint_costs,
            handle_payment_costs,
            standard_payment_costs,
        }
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
}

#[cfg(any(feature = "testing", test))]
impl SystemConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        // there's a bug in toml...under the hood it uses an i64 when it should use a u64
        // this causes flaky test failures if the random result exceeds i64::MAX
        let auction_costs: AuctionCosts = rng.gen();
        let mint_costs = rng.gen();
        let handle_payment_costs = rng.gen();
        let standard_payment_costs = rng.gen();

        SystemConfig {
            auction_costs,
            mint_costs,
            handle_payment_costs,
            standard_payment_costs,
        }
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            auction_costs: AuctionCosts::default(),
            mint_costs: MintCosts::default(),
            handle_payment_costs: HandlePaymentCosts::default(),
            standard_payment_costs: StandardPaymentCosts::default(),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<SystemConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SystemConfig {
        SystemConfig {
            auction_costs: rng.gen(),
            mint_costs: rng.gen(),
            handle_payment_costs: rng.gen(),
            standard_payment_costs: rng.gen(),
        }
    }
}

impl ToBytes for SystemConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.auction_costs.to_bytes()?);
        ret.append(&mut self.mint_costs.to_bytes()?);
        ret.append(&mut self.handle_payment_costs.to_bytes()?);
        ret.append(&mut self.standard_payment_costs.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.auction_costs.serialized_length()
            + self.mint_costs.serialized_length()
            + self.handle_payment_costs.serialized_length()
            + self.standard_payment_costs.serialized_length()
    }
}

impl FromBytes for SystemConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (auction_costs, rem) = FromBytes::from_bytes(bytes)?;
        let (mint_costs, rem) = FromBytes::from_bytes(rem)?;
        let (handle_payment_costs, rem) = FromBytes::from_bytes(rem)?;
        let (standard_payment_costs, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            SystemConfig::new(
                auction_costs,
                mint_costs,
                handle_payment_costs,
                standard_payment_costs,
            ),
            rem,
        ))
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prop_compose;

    use crate::{
        chainspec::vm_config::{
            auction_costs::gens::auction_costs_arb,
            handle_payment_costs::gens::handle_payment_costs_arb, mint_costs::gens::mint_costs_arb,
            standard_payment_costs::gens::standard_payment_costs_arb,
        },
        SystemConfig,
    };

    prop_compose! {
        pub fn system_config_arb()(
            auction_costs in auction_costs_arb(),
            mint_costs in mint_costs_arb(),
            handle_payment_costs in handle_payment_costs_arb(),
            standard_payment_costs in standard_payment_costs_arb(),
        ) -> SystemConfig {
            SystemConfig {
                auction_costs,
                mint_costs,
                handle_payment_costs,
                standard_payment_costs,
            }
        }
    }
}
