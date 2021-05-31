use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::system_config::handle_payment_costs::HandlePaymentCosts;

pub struct LegacyHandlePaymentCosts(HandlePaymentCosts);

impl From<LegacyHandlePaymentCosts> for HandlePaymentCosts {
    fn from(legacy_handle_payment_costs: LegacyHandlePaymentCosts) -> Self {
        legacy_handle_payment_costs.0
    }
}

impl FromBytes for LegacyHandlePaymentCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (get_payment_purse, rem) = FromBytes::from_bytes(bytes)?;
        let (set_refund_purse, rem) = FromBytes::from_bytes(rem)?;
        let (get_refund_purse, rem) = FromBytes::from_bytes(rem)?;
        let (finalize_payment, rem) = FromBytes::from_bytes(rem)?;
        let handle_payment_costs = HandlePaymentCosts {
            get_payment_purse,
            set_refund_purse,
            get_refund_purse,
            finalize_payment,
        };

        Ok((LegacyHandlePaymentCosts(handle_payment_costs), rem))
    }
}
