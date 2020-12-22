use crate::{account::AccountHash, system_contract_errors::mint::Error, Key};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// This method should handle storing given [`Key`] under `name`.
    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error>;

    /// This method should handle obtaining a given named [`Key`] under a `name`.
    fn get_key(&self, name: &str) -> Option<Key>;
}
