use crate::{
    bytesrepr::{FromBytes, ToBytes},
    system::mint::Error,
    CLTyped, Key, URef,
};

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    /// Create new [`URef`].
    fn new_uref<T: CLTyped + ToBytes>(&mut self, init: T) -> Result<URef, Error>;

    /// Write data to a local key.
    fn write_balance_entry(&mut self, purse_uref: URef, balance_uref: URef) -> Result<(), Error>;

    /// Read data from a local key.
    fn read_balance_entry(&mut self, purse_uref: &URef) -> Result<Option<Key>, Error>;

    /// Read data from [`URef`].
    fn read<T: CLTyped + FromBytes>(&mut self, uref: URef) -> Result<Option<T>, Error>;

    /// Write data under a [`URef`].
    fn write<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error>;

    /// Add data to a [`URef`].
    fn add<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error>;
}
