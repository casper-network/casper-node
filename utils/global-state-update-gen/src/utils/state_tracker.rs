use std::collections::BTreeMap;

use rand::Rng;

use casper_types::{AccessRights, CLValue, Key, StoredValue, URef, U512};

use crate::utils::print_entry;

/// A struct tracking changes to be made to the global state.
pub struct StateTracker {
    entries_to_write: BTreeMap<Key, StoredValue>,
    total_supply: U512,
    total_supply_key: Key,
}

impl StateTracker {
    /// Creates a new `StateTracker`.
    pub fn new(total_supply_key: Key, total_supply: U512) -> Self {
        Self {
            entries_to_write: Default::default(),
            total_supply_key,
            total_supply,
        }
    }

    /// Prints all the writes to be made to the global state.
    pub fn print_all_entries(&self) {
        for (key, value) in &self.entries_to_write {
            print_entry(key, value);
        }
    }

    /// Stores a write of an entry in the global state.
    pub fn write_entry(&mut self, key: Key, value: StoredValue) {
        let _ = self.entries_to_write.insert(key, value);
    }

    /// Increases the total supply of the tokens in the network.
    pub fn increase_supply(&mut self, to_add: U512) {
        self.total_supply += to_add;
        self.write_entry(
            self.total_supply_key,
            StoredValue::CLValue(CLValue::from_t(self.total_supply).unwrap()),
        );
    }

    /// Decreases the total supply of the tokens in the network.
    pub fn decrease_supply(&mut self, to_sub: U512) {
        self.total_supply -= to_sub;
        self.write_entry(
            self.total_supply_key,
            StoredValue::CLValue(CLValue::from_t(self.total_supply).unwrap()),
        );
    }

    /// Creates a new purse containing the given amount of motes and returns its URef.
    pub fn create_purse(&mut self, amount: U512) -> URef {
        let mut rng = rand::thread_rng();
        let new_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);

        // Purse URef pointing to `()` so that the owner cannot modify the purse directly.
        self.write_entry(Key::URef(new_purse), StoredValue::CLValue(CLValue::unit()));

        // This will create a new purse by storing a zero balance in it.
        self.write_entry(
            Key::Balance(new_purse.addr()),
            StoredValue::CLValue(CLValue::from_t(amount).unwrap()),
        );

        self.increase_supply(amount);

        new_purse
    }
}
