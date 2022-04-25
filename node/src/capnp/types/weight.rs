use casper_types::account::Weight;

use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

use super::{FromCapnpReader, ToCapnpBuilder};

#[allow(dead_code)]
pub(super) mod weight_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/weight_capnp.rs"
    ));
}

impl ToCapnpBuilder<Weight> for weight_capnp::weight::Builder<'_> {
    fn try_to_builder(&mut self, weight: &Weight) -> Result<(), SerializeError> {
        let mut weight_builder = self.reborrow();
        weight_builder.set_value(weight.value());
        Ok(())
    }
}

impl FromCapnpReader<Weight> for weight_capnp::weight::Reader<'_> {
    fn try_from_reader(&self) -> Result<Weight, DeserializeError> {
        let weight = self.get_value();
        Ok(Weight::new(weight))
    }
}

impl ToCapnpBytes for Weight {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<weight_capnp::weight::Builder>();
        msg.try_to_builder(self)?;
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
        reader.try_from_reader()
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
