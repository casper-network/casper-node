mod auction_costs;
mod handle_payment_costs;
mod mint_costs;
mod standard_payment_costs;

use casper_types::bytesrepr::FromBytes;

use crate::shared::system_config::SystemConfig;

use self::{
    auction_costs::LegacyAuctionCosts, handle_payment_costs::LegacyHandlePaymentCosts,
    mint_costs::LegacyMintCosts, standard_payment_costs::LegacyStandardPaymentCosts,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LegacySystemConfig(SystemConfig);

impl From<LegacySystemConfig> for SystemConfig {
    fn from(legacy_system_config: LegacySystemConfig) -> Self {
        legacy_system_config.0
    }
}

impl FromBytes for LegacySystemConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (wasmless_transfer_cost, rem) = u32::from_bytes(bytes)?;
        let (legacy_auction_costs, rem) = LegacyAuctionCosts::from_bytes(rem)?;
        let (legacy_mint_costs, rem) = LegacyMintCosts::from_bytes(rem)?;
        let (legacy_handle_payment_costs, rem) = LegacyHandlePaymentCosts::from_bytes(rem)?;
        let (legacy_standard_payment_costs, rem) = LegacyStandardPaymentCosts::from_bytes(rem)?;

        let system_config = SystemConfig::new(
            wasmless_transfer_cost,
            legacy_auction_costs.into(),
            legacy_mint_costs.into(),
            legacy_handle_payment_costs.into(),
            legacy_standard_payment_costs.into(),
        );

        Ok((LegacySystemConfig(system_config), rem))
    }
}
