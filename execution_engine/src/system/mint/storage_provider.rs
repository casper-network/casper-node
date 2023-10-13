use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    system::mint::Error,
    CLTyped, Key, URef, U512,
};

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    /// Create new [`URef`].
    fn new_uref<T: CLTyped + ToBytes>(&mut self, init: T) -> Result<URef, Error>;

    /// Read data from [`URef`].
    fn read<T: CLTyped + FromBytes>(&mut self, key: Key) -> Result<Option<T>, Error>;

    /// Write data under a [`URef`].
    fn write<T: CLTyped + ToBytes>(&mut self, key: Key, value: T) -> Result<(), Error>;

    /// Add data to a [`URef`].
    fn add<T: CLTyped + ToBytes>(&mut self, key: Key, value: T) -> Result<(), Error>;

    /// Read balance.
    fn read_balance(&mut self, uref: URef) -> Result<Option<U512>, Error>;

    /// Write balance.
    fn write_balance(&mut self, uref: URef, balance: U512) -> Result<(), Error>;

    /// Add amount to an existing balance.
    fn add_balance(&mut self, uref: URef, value: U512) -> Result<(), Error>;
}
