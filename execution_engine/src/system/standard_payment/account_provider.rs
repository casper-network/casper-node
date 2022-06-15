use casper_types::{ApiError, URef};

/// Provider of an account related functionality.
pub trait AccountProvider {
    /// Get currently executing account's purse.
    fn get_main_purse(&mut self) -> Result<URef, ApiError>;
}
