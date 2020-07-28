use casperlabs_types::{
    bytesrepr::{FromBytes, ToBytes},
    CLTyped, Key, URef, U512,
};

pub trait StorageProvider {
    type Error;

    fn get_key(&mut self, name: &str) -> Option<Key>;
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Self::Error>;
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T);
}

pub trait ProofOfStakeProvider {
    fn bond(&mut self, amount: U512, purse: URef);
    fn unbond(&mut self, amount: Option<U512>);
}

pub trait SystemProvider {
    type Error;
    fn create_purse(&mut self) -> URef;
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Self::Error>;
}
