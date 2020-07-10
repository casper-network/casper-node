use crate::{account::AccountHash, Key};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// This method should handle storing given [`Key`] under `name`.
    fn put_key(&mut self, name: &str, key: Key);
}
