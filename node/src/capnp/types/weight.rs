use casper_types::account::Weight;

use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

#[allow(dead_code)]
mod weight_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/weight_capnp.rs"
    ));
}

impl ToCapnpBytes for Weight {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        {
            let mut msg = builder.init_root::<weight_capnp::weight::Builder>();
            msg.set_value(self.value());
        }
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for Weight {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;
        let reader = deserialized.get_root::<weight_capnp::weight::Reader>()?;
        Ok(Self::new(reader.get_value()))
    }
}

#[cfg(test)]
mod tests {
    use casper_types::account::Weight;

    use crate::capnp::{FromCapnpBytes, ToCapnpBytes};

    #[test]
    fn test_capnp() {
        const WEIGHT: u8 = 175;

        let original = Weight::new(WEIGHT);
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = Weight::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
