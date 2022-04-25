use casper_types::account::{AccountHash, ACCOUNT_HASH_LENGTH};

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};
use casper_node_macros::make_capnp_byte_setter_functions;

#[allow(dead_code)]
pub(super) mod account_hash_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/account_hash_capnp.rs"
    ));
}

// const ACCOUNT_HASH_LENGTH = 32;
make_capnp_byte_setter_functions!(32, "account_hash", "account_hash_capnp::account_hash");

impl ToCapnpBuilder<AccountHash> for account_hash_capnp::account_hash::Builder<'_> {
    fn try_to_builder(&mut self, account_hash: &AccountHash) -> Result<(), SerializeError> {
        let bytes = account_hash.value();
        let mut msg = self.reborrow();
        set_account_hash(&mut msg, &bytes);
        Ok(())
    }
}

impl FromCapnpReader<AccountHash> for account_hash_capnp::account_hash::Reader<'_> {
    fn try_from_reader(&self) -> Result<AccountHash, DeserializeError> {
        let bytes: [u8; ACCOUNT_HASH_LENGTH] = get_account_hash(*self);
        Ok(AccountHash::new(bytes))
    }
}

impl ToCapnpBytes for AccountHash {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<account_hash_capnp::account_hash::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for AccountHash {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;

        let reader = deserialized.get_root::<account_hash_capnp::account_hash::Reader>()?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use casper_types::account::{AccountHash, ACCOUNT_HASH_LENGTH};

    use crate::capnp::{types::random_bytes, FromCapnpBytes, ToCapnpBytes};

    pub(crate) fn random_account_hash() -> AccountHash {
        let mut account_hash: [u8; ACCOUNT_HASH_LENGTH] = Default::default();
        account_hash.copy_from_slice(&random_bytes(ACCOUNT_HASH_LENGTH));

        AccountHash::new(account_hash)
    }

    #[test]
    fn account_hash_capnp() {
        let account_hash = random_account_hash();
        let original = account_hash;
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = AccountHash::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
