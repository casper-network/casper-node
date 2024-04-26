use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::entity::{
        runtime_provider::RuntimeProvider, storage_provider::StorageProvider, Entity,
    },
};
use casper_types::{
    account::AccountHash, addressable_entity::Weight, system::entity::Error, AddressableEntity,
    Key, StoredValue,
};

use super::Runtime;
use crate::execution::ExecError;

impl From<ExecError> for Option<Error> {
    fn from(exec_error: ExecError) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure [`Auction`]
            // contract running natively supports propagating gas limit errors without a panic.
            ExecError::GasLimit => Some(Error::BuggerAll), // GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn entity_key(&self) -> Result<&AddressableEntity, Error> {
        unimplemented!()
    }
}

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn read_key(&mut self, key: &Key) -> Result<Option<AddressableEntity>, Error> {
        unimplemented!()
    }

    fn write_key(&mut self, key: Key, value: StoredValue) -> Result<(), Error> {
        unimplemented!()
    }
}

impl<'a, R> Entity for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), Error> {
        let caller = self.context.get_caller();
        self.context
            .add_associated_key(account_hash, weight)
            .unwrap();
        Ok(())
    }
}
