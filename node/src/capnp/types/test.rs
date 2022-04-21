use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

#[allow(dead_code)]
mod test_capnp {
    include!(concat!(env!("OUT_DIR"), "/src/capnp/schemas/test_capnp.rs"));
}

#[derive(Debug, Eq, PartialEq)]
struct Test {
    id: i8,
}

impl ToCapnpBytes for Test {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        {
            let mut msg = builder.init_root::<test_capnp::test::Builder>();
            msg.set_id(self.id);
        }
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for Test {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;
        let reader = deserialized.get_root::<test_capnp::test::Reader>()?;
        Ok(Self {
            id: reader.get_id(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::capnp::{types::test::Test, FromCapnpBytes, ToCapnpBytes};

    #[test]
    fn test_capnp() {
        const ID: i8 = -109;

        let original = Test { id: ID };
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = Test::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
