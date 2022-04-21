use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};
use casper_hashing::Digest;
use casper_node_macros::make_capnp_byte_setter_functions;

#[allow(dead_code)]
pub(super) mod digest_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/digest_capnp.rs"
    ));
}

// We cannot use the const literals directly
// const Digest::LENGTH = 32;
make_capnp_byte_setter_functions!(32, "digest", "digest_capnp::digest");

impl ToCapnpBuilder<Digest> for digest_capnp::digest::Builder<'_> {
    fn try_to_builder(&mut self, digest: &Digest) -> Result<(), SerializeError> {
        let bytes = digest.value();
        set_digest(&mut self.reborrow(), &bytes);
        Ok(())
    }
}

impl FromCapnpReader<Digest> for digest_capnp::digest::Reader<'_> {
    fn try_from_reader(&self) -> Result<Digest, DeserializeError> {
        let bytes: [u8; Digest::LENGTH] = get_digest(*self);
        Ok(bytes.into())
    }
}

impl ToCapnpBytes for Digest {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<digest_capnp::digest::Builder>();
        msg.try_to_builder(self)?;

        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for Digest {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;

        let reader = deserialized.get_root::<digest_capnp::digest::Reader>()?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{super::random_bytes, *};

    use std::convert::TryFrom;

    pub(crate) fn random_digest() -> Digest {
        let bytes = random_bytes(Digest::LENGTH);
        Digest::try_from(bytes.as_slice()).unwrap()
    }

    #[test]
    fn digest_capnp() {
        let digest: Digest = random_digest();
        let original = digest;
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = Digest::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
