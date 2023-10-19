use casper_types::{ApiError, URef};

/// Provider of handle payment functionality.
pub trait HandlePaymentProvider {
    /// Get payment purse for given deploy.
    fn get_payment_purse(&mut self) -> Result<URef, ApiError>;
}
