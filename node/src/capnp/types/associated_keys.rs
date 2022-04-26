use std::convert::TryInto;

use casper_types::account::AssociatedKeys;

use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

use super::{FromCapnpReader, ToCapnpBuilder};

#[allow(dead_code)]
pub(super) mod associated_keys_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/associated_keys_capnp.rs"
    ));
}

impl ToCapnpBuilder<AssociatedKeys> for associated_keys_capnp::associated_keys::Builder<'_> {
    fn try_to_builder(&mut self, associated_keys: &AssociatedKeys) -> Result<(), SerializeError> {
        let associated_keys_count: u32 = associated_keys
            .len()
            .try_into()
            .map_err(|_| SerializeError::TooManyItems)?;

        let mut list_builder = self.reborrow().init_value();
        let mut associated_key_builder =
            list_builder.reborrow().init_entries(associated_keys_count);
        for (index, (key, value)) in associated_keys.iter().enumerate() {
            let mut entry_builder = associated_key_builder.reborrow().get(index as u32);
            {
                let mut msg = entry_builder.reborrow().init_key();
                msg.try_to_builder(key)?;
            }
            {
                let mut msg = entry_builder.reborrow().init_value();
                msg.try_to_builder(value)?;
            }
        }
        Ok(())
    }
}

impl FromCapnpReader<AssociatedKeys> for associated_keys_capnp::associated_keys::Reader<'_> {
    fn try_from_reader(&self) -> Result<AssociatedKeys, DeserializeError> {
        let mut associated_keys = AssociatedKeys::default();
        if self.has_value() {
            let entries_reader = self.get_value()?;
            for entry_reader in entries_reader.get_entries()?.iter() {
                let key = entry_reader.get_key()?.try_from_reader()?;
                let value = entry_reader.get_value()?.try_from_reader()?;
                associated_keys.add_key(key, value)?;
            }
        }
        Ok(associated_keys)
    }
}

impl ToCapnpBytes for AssociatedKeys {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<associated_keys_capnp::associated_keys::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for AssociatedKeys {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;

        let reader = deserialized.get_root::<associated_keys_capnp::associated_keys::Reader>()?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use casper_types::account::{AssociatedKeys, Weight};

    use crate::capnp::{
        types::{account_hash::tests::random_account_hash, random_byte},
        FromCapnpBytes, ToCapnpBytes,
    };

    #[test]
    fn test_capnp() {
        const KEY_COUNT: usize = 1000;

        // We need `KEY_COUNT` distinct keys.
        // It's hardly possible that random will create duplicates, but
        // let's prevent that anyway for a good measure.
        let mut keys = BTreeSet::new();
        while keys.len() != KEY_COUNT {
            keys.insert(random_account_hash());
        }

        let mut original = AssociatedKeys::default();
        keys.into_iter().for_each(|key| {
            original
                .add_key(key, Weight::new(random_byte()))
                .expect("should add key");
        });

        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized =
            AssociatedKeys::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
