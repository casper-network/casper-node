use crate::{
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::Error,
    CLTyped, Key, URef, U512,
};

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    /// Compatible error type.
    type Error: From<Error>;

    /// Obtain named [`Key`].
    fn get_key(&mut self, name: &str) -> Option<Key>;

    /// Read data from [`URef`].
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Self::Error>;

    /// Write data to [`URef].
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Self::Error>;
}

/// Provides functionality of a proof of stake contract.
pub trait ProofOfStakeProvider {
    /// Error representation for PoS errors.
    type Error: From<Error>;

    /// Bond specified amount from a purse.
    fn bond(&mut self, amount: U512, purse: URef) -> Result<(), Self::Error>;

    /// Unbond specified amount.
    fn unbond(&mut self, amount: Option<U512>) -> Result<(), Self::Error>;
}

/// Provides functionality of a system.
pub trait SystemProvider {
    /// Error represntation for system errors.
    type Error: From<Error>;

    /// Creates new purse.
    fn create_purse(&mut self) -> URef;

    /// Transfers specified `amount` of tokens from `source` purse into a `target` purse.
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Self::Error>;
}
