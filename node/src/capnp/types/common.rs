use casper_types::U512;

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{Error, FromCapnpBytes, ToCapnpBytes};

#[allow(dead_code)]
pub(super) mod common_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/common_capnp.rs"
    ));
}

impl ToCapnpBuilder<U512> for common_capnp::u512::Builder<'_> {
    fn try_to_builder(&mut self, x: &U512) -> Result<(), Error> {
        let mut le_bytes = [0u8; 64];
        x.to_little_endian(&mut le_bytes[..]);
        self.set_byte0(le_bytes[0]);
        self.set_byte1(le_bytes[1]);
        self.set_byte2(le_bytes[2]);
        self.set_byte3(le_bytes[3]);
        self.set_byte4(le_bytes[4]);
        self.set_byte5(le_bytes[5]);
        self.set_byte6(le_bytes[6]);
        self.set_byte7(le_bytes[7]);
        self.set_byte8(le_bytes[8]);
        self.set_byte9(le_bytes[9]);
        self.set_byte10(le_bytes[10]);
        self.set_byte11(le_bytes[11]);
        self.set_byte12(le_bytes[12]);
        self.set_byte13(le_bytes[13]);
        self.set_byte14(le_bytes[14]);
        self.set_byte15(le_bytes[15]);
        self.set_byte16(le_bytes[16]);
        self.set_byte17(le_bytes[17]);
        self.set_byte18(le_bytes[18]);
        self.set_byte19(le_bytes[19]);
        self.set_byte20(le_bytes[20]);
        self.set_byte21(le_bytes[21]);
        self.set_byte22(le_bytes[22]);
        self.set_byte23(le_bytes[23]);
        self.set_byte24(le_bytes[24]);
        self.set_byte25(le_bytes[25]);
        self.set_byte26(le_bytes[26]);
        self.set_byte27(le_bytes[27]);
        self.set_byte28(le_bytes[28]);
        self.set_byte29(le_bytes[29]);
        self.set_byte30(le_bytes[30]);
        self.set_byte31(le_bytes[31]);
        self.set_byte32(le_bytes[32]);
        self.set_byte33(le_bytes[33]);
        self.set_byte34(le_bytes[34]);
        self.set_byte35(le_bytes[35]);
        self.set_byte36(le_bytes[36]);
        self.set_byte37(le_bytes[37]);
        self.set_byte38(le_bytes[38]);
        self.set_byte39(le_bytes[39]);
        self.set_byte40(le_bytes[40]);
        self.set_byte41(le_bytes[41]);
        self.set_byte42(le_bytes[42]);
        self.set_byte43(le_bytes[43]);
        self.set_byte44(le_bytes[44]);
        self.set_byte45(le_bytes[45]);
        self.set_byte46(le_bytes[46]);
        self.set_byte47(le_bytes[47]);
        self.set_byte48(le_bytes[48]);
        self.set_byte49(le_bytes[49]);
        self.set_byte50(le_bytes[50]);
        self.set_byte51(le_bytes[51]);
        self.set_byte52(le_bytes[52]);
        self.set_byte53(le_bytes[53]);
        self.set_byte54(le_bytes[54]);
        self.set_byte55(le_bytes[55]);
        self.set_byte56(le_bytes[56]);
        self.set_byte57(le_bytes[57]);
        self.set_byte58(le_bytes[58]);
        self.set_byte59(le_bytes[59]);
        self.set_byte60(le_bytes[60]);
        self.set_byte61(le_bytes[61]);
        self.set_byte62(le_bytes[62]);
        self.set_byte63(le_bytes[63]);
        Ok(())
    }
}

impl FromCapnpReader<U512> for common_capnp::u512::Reader<'_> {
    fn try_from_reader(&self) -> Result<U512, Error> {
        let mut le_bytes = [0u8; 64];
        le_bytes[0] = self.get_byte0();
        le_bytes[1] = self.get_byte1();
        le_bytes[2] = self.get_byte2();
        le_bytes[3] = self.get_byte3();
        le_bytes[4] = self.get_byte4();
        le_bytes[5] = self.get_byte5();
        le_bytes[6] = self.get_byte6();
        le_bytes[7] = self.get_byte7();
        le_bytes[8] = self.get_byte8();
        le_bytes[9] = self.get_byte9();
        le_bytes[10] = self.get_byte10();
        le_bytes[11] = self.get_byte11();
        le_bytes[12] = self.get_byte12();
        le_bytes[13] = self.get_byte13();
        le_bytes[14] = self.get_byte14();
        le_bytes[15] = self.get_byte15();
        le_bytes[16] = self.get_byte16();
        le_bytes[17] = self.get_byte17();
        le_bytes[18] = self.get_byte18();
        le_bytes[19] = self.get_byte19();
        le_bytes[20] = self.get_byte20();
        le_bytes[21] = self.get_byte21();
        le_bytes[22] = self.get_byte22();
        le_bytes[23] = self.get_byte23();
        le_bytes[24] = self.get_byte24();
        le_bytes[25] = self.get_byte25();
        le_bytes[26] = self.get_byte26();
        le_bytes[27] = self.get_byte27();
        le_bytes[28] = self.get_byte28();
        le_bytes[29] = self.get_byte29();
        le_bytes[30] = self.get_byte30();
        le_bytes[31] = self.get_byte31();
        le_bytes[32] = self.get_byte32();
        le_bytes[33] = self.get_byte33();
        le_bytes[34] = self.get_byte34();
        le_bytes[35] = self.get_byte35();
        le_bytes[36] = self.get_byte36();
        le_bytes[37] = self.get_byte37();
        le_bytes[38] = self.get_byte38();
        le_bytes[39] = self.get_byte39();
        le_bytes[40] = self.get_byte40();
        le_bytes[41] = self.get_byte41();
        le_bytes[42] = self.get_byte42();
        le_bytes[43] = self.get_byte43();
        le_bytes[44] = self.get_byte44();
        le_bytes[45] = self.get_byte45();
        le_bytes[46] = self.get_byte46();
        le_bytes[47] = self.get_byte47();
        le_bytes[48] = self.get_byte48();
        le_bytes[49] = self.get_byte49();
        le_bytes[50] = self.get_byte50();
        le_bytes[51] = self.get_byte51();
        le_bytes[52] = self.get_byte52();
        le_bytes[53] = self.get_byte53();
        le_bytes[54] = self.get_byte54();
        le_bytes[55] = self.get_byte55();
        le_bytes[56] = self.get_byte56();
        le_bytes[57] = self.get_byte57();
        le_bytes[58] = self.get_byte58();
        le_bytes[59] = self.get_byte59();
        le_bytes[60] = self.get_byte60();
        le_bytes[61] = self.get_byte61();
        le_bytes[62] = self.get_byte62();
        le_bytes[63] = self.get_byte63();
        Ok(U512::from_little_endian(&le_bytes))
    }
}

impl ToCapnpBytes for U512 {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<common_capnp::u512::Builder>();
        msg.try_to_builder(self)?;

        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)
            .map_err(|_| Error::UnableToSerialize)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for U512 {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .expect("unable to deserialize struct");

        let reader = deserialized
            .get_root::<common_capnp::u512::Reader>()
            .map_err(|_| Error::UnableToDeserialize)?;
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
