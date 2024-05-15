use casper_types::account::AccountHash;

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller_new(&self) -> AccountHash;
}
