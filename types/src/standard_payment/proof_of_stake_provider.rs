use crate::{ApiError, URef};

/// Provider of proof of stake functionality.
pub trait ProofOfStakeProvider {
    /// Get payment purse for given deploy.
    fn get_payment_purse(&mut self) -> Result<URef, ApiError>;
}
