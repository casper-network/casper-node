use casper_types::U512;

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};
use casper_node_macros::make_u512_capnp_functions;

#[allow(dead_code)]
pub(super) mod common_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/common_capnp.rs"
    ));
}

make_u512_capnp_functions!();

impl ToCapnpBuilder<U512> for common_capnp::u512::Builder<'_> {
    fn try_to_builder(&mut self, x: &U512) -> Result<(), SerializeError> {
        let mut le_bytes = [0u8; 64];
        x.to_little_endian(&mut le_bytes[..]);
        let mut msg = self.reborrow();
        set_u512(&mut msg, &le_bytes);
        Ok(())
    }
}

impl FromCapnpReader<U512> for common_capnp::u512::Reader<'_> {
    fn try_from_reader(&self) -> Result<U512, DeserializeError> {
        let le_bytes = get_u512(self);
        Ok(U512::from_little_endian(&le_bytes))
    }
}

impl ToCapnpBytes for U512 {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<common_capnp::u512::Builder>();
        msg.try_to_builder(self)?;

        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for U512 {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;

        let reader = deserialized
            .get_root::<common_capnp::u512::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::random_bytes;
    use super::*;

    pub(crate) fn random_u512() -> U512 {
        let bytes = random_bytes(64);
        U512::from_little_endian(&bytes)
    }

    #[test]
    fn u512_capnp() {
        let x = random_u512();
        let original = x.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = U512::try_from_capnp_bytes(&serialized).expect("deserialization");
        assert_eq!(original, deserialized);
    }
}
