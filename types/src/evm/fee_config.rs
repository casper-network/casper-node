use num_rational::Ratio;

use crate::{U256, U512};

/// The subset of the EVM chainspec configuration needed to price EVM transactions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct EvmFeeConfig {
    /// Base fee denominated in motes per EVM gas.
    base_fee: u64,
    /// Number of wei represented by one mote.
    wei_per_mote: u64,
}

impl EvmFeeConfig {
    /// Creates a new `EvmFeeConfig`.
    pub fn new(base_fee: u64, wei_per_mote: u64) -> Self {
        EvmFeeConfig {
            base_fee,
            wei_per_mote,
        }
    }

    /// Returns the base fee denominated in motes per EVM gas.
    pub fn base_fee(&self) -> u64 {
        self.base_fee
    }

    /// Returns the number of wei represented by one mote.
    pub fn wei_per_mote(&self) -> u64 {
        self.wei_per_mote
    }

    /// Returns the EVM base fee denominated in wei.
    pub fn base_fee_wei(&self) -> u128 {
        u128::from(self.base_fee) * u128::from(self.wei_per_mote)
    }

    /// Converts an EVM gas cost, denominated in wei, to motes by rounding up.
    ///
    /// Rounding is applied after multiplying gas by price, so sub-mote totals
    /// are charged as one mote without overcharging each gas unit separately.
    pub fn gas_fee_motes(&self, gas: u64, gas_price_wei: u128) -> Option<U512> {
        if self.wei_per_mote == 0 {
            return None;
        }
        let fee_wei = U512::from(gas).checked_mul(U512::from(gas_price_wei))?;
        Some(
            Ratio::new(fee_wei, U512::from(self.wei_per_mote))
                .ceil()
                .to_integer(),
        )
    }

    /// Converts an Ethereum transaction value from wei to motes.
    ///
    /// Casper purse balances have mote precision, so values containing a
    /// fractional mote are not representable and return `None`.
    pub fn value_motes(&self, value_wei: U256) -> Option<U256> {
        if self.wei_per_mote == 0 {
            return None;
        }
        let wei_per_mote = U256::from(self.wei_per_mote);
        if value_wei % wei_per_mote != U256::zero() {
            return None;
        }
        Some(value_wei / wei_per_mote)
    }
}
