use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::system_config::standard_payment_costs::StandardPaymentCosts;

pub struct LegacyStandardPaymentCosts(StandardPaymentCosts);

impl From<LegacyStandardPaymentCosts> for StandardPaymentCosts {
    fn from(legacy_standard_payment_costs: LegacyStandardPaymentCosts) -> Self {
        legacy_standard_payment_costs.0
    }
}

impl FromBytes for LegacyStandardPaymentCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (pay, rem) = FromBytes::from_bytes(bytes)?;
        let standard_payment_costs = StandardPaymentCosts { pay };
        Ok((LegacyStandardPaymentCosts(standard_payment_costs), rem))
    }
}
