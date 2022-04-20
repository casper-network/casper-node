use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};
use casper_hashing::Digest;

#[allow(dead_code)]
pub(super) mod digest_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/digest_capnp.rs"
    ));
}

impl ToCapnpBuilder<Digest> for digest_capnp::digest::Builder<'_> {
    fn try_to_builder(&mut self, digest: &Digest) -> Result<(), SerializeError> {
        let bytes = digest.value();
        self.set_byte0(bytes[0]);
        self.set_byte1(bytes[1]);
        self.set_byte2(bytes[2]);
        self.set_byte3(bytes[3]);
        self.set_byte4(bytes[4]);
        self.set_byte5(bytes[5]);
        self.set_byte6(bytes[6]);
        self.set_byte7(bytes[7]);
        self.set_byte8(bytes[8]);
        self.set_byte9(bytes[9]);
        self.set_byte10(bytes[10]);
        self.set_byte11(bytes[11]);
        self.set_byte12(bytes[12]);
        self.set_byte13(bytes[13]);
        self.set_byte14(bytes[14]);
        self.set_byte15(bytes[15]);
        self.set_byte16(bytes[16]);
        self.set_byte17(bytes[17]);
        self.set_byte18(bytes[18]);
        self.set_byte19(bytes[19]);
        self.set_byte20(bytes[20]);
        self.set_byte21(bytes[20]);
        self.set_byte21(bytes[21]);
        self.set_byte22(bytes[22]);
        self.set_byte23(bytes[23]);
        self.set_byte24(bytes[24]);
        self.set_byte25(bytes[25]);
        self.set_byte26(bytes[26]);
        self.set_byte27(bytes[27]);
        self.set_byte28(bytes[28]);
        self.set_byte29(bytes[29]);
        self.set_byte30(bytes[30]);
        self.set_byte31(bytes[31]);
        Ok(())
    }
}

impl FromCapnpReader<Digest> for digest_capnp::digest::Reader<'_> {
    fn try_from_reader(&self) -> Result<Digest, DeserializeError> {
        let bytes: [u8; Digest::LENGTH] = [
            self.get_byte0(),
            self.get_byte1(),
            self.get_byte2(),
            self.get_byte3(),
            self.get_byte4(),
            self.get_byte5(),
            self.get_byte6(),
            self.get_byte7(),
            self.get_byte8(),
            self.get_byte9(),
            self.get_byte10(),
            self.get_byte11(),
            self.get_byte12(),
            self.get_byte13(),
            self.get_byte14(),
            self.get_byte15(),
            self.get_byte16(),
            self.get_byte17(),
            self.get_byte18(),
            self.get_byte19(),
            self.get_byte20(),
            self.get_byte21(),
            self.get_byte22(),
            self.get_byte23(),
            self.get_byte24(),
            self.get_byte25(),
            self.get_byte26(),
            self.get_byte27(),
            self.get_byte28(),
            self.get_byte29(),
            self.get_byte30(),
            self.get_byte31(),
        ];

        Ok(bytes.into())
    }
}

impl ToCapnpBytes for Digest {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<digest_capnp::digest::Builder>();
        msg.try_to_builder(self)?;

        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for Digest {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;

        let reader = deserialized
            .get_root::<digest_capnp::digest::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::random_bytes;
    use super::*;

    use std::convert::TryFrom;

    pub(crate) fn random_digest() -> Digest {
        let bytes = random_bytes(Digest::LENGTH);
        Digest::try_from(bytes.as_slice()).unwrap()
    }

    #[test]
    fn digest_capnp() {
        let digest: Digest = random_digest();
        let original = digest.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = Digest::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
