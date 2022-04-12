use alloc::vec::Vec;

use crate::{AsymmetricType, PublicKey};

use super::{Error, FromCapnpBytes, ToCapnpBytes};

#[allow(dead_code)]
mod public_key_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/schemas/capnp/public_key_capnp.rs"
    ));
}

impl ToCapnpBytes for PublicKey {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut builder = capnp::message::Builder::new_default();
        match self {
            PublicKey::Ed25519(key) => {
                let root = builder.init_root::<public_key_capnp::public_key::Builder>();
                let bytes = key.as_bytes();
                let mut msg = root.init_ed25519();
                msg.set_byte0(bytes[0]);
                msg.set_byte1(bytes[1]);
                msg.set_byte2(bytes[2]);
                msg.set_byte3(bytes[3]);
                msg.set_byte4(bytes[4]);
                msg.set_byte5(bytes[5]);
                msg.set_byte6(bytes[6]);
                msg.set_byte7(bytes[7]);
                msg.set_byte8(bytes[8]);
                msg.set_byte9(bytes[9]);
                msg.set_byte10(bytes[10]);
                msg.set_byte11(bytes[11]);
                msg.set_byte12(bytes[12]);
                msg.set_byte13(bytes[13]);
                msg.set_byte14(bytes[14]);
                msg.set_byte15(bytes[15]);
                msg.set_byte16(bytes[16]);
                msg.set_byte17(bytes[17]);
                msg.set_byte18(bytes[18]);
                msg.set_byte19(bytes[19]);
                msg.set_byte20(bytes[20]);
                msg.set_byte21(bytes[20]);
                msg.set_byte21(bytes[21]);
                msg.set_byte22(bytes[22]);
                msg.set_byte23(bytes[23]);
                msg.set_byte24(bytes[24]);
                msg.set_byte25(bytes[25]);
                msg.set_byte26(bytes[26]);
                msg.set_byte27(bytes[27]);
                msg.set_byte28(bytes[28]);
                msg.set_byte29(bytes[29]);
                msg.set_byte30(bytes[30]);
                msg.set_byte31(bytes[31]);
            }
            PublicKey::Secp256k1(key) => {
                let root = builder.init_root::<public_key_capnp::public_key::Builder>();
                let bytes = key.to_bytes();
                let mut msg = root.init_secp256k1();
                msg.set_byte0(bytes[0]);
                msg.set_byte1(bytes[1]);
                msg.set_byte2(bytes[2]);
                msg.set_byte3(bytes[3]);
                msg.set_byte4(bytes[4]);
                msg.set_byte5(bytes[5]);
                msg.set_byte6(bytes[6]);
                msg.set_byte7(bytes[7]);
                msg.set_byte8(bytes[8]);
                msg.set_byte9(bytes[9]);
                msg.set_byte10(bytes[10]);
                msg.set_byte11(bytes[11]);
                msg.set_byte12(bytes[12]);
                msg.set_byte13(bytes[13]);
                msg.set_byte14(bytes[14]);
                msg.set_byte15(bytes[15]);
                msg.set_byte16(bytes[16]);
                msg.set_byte17(bytes[17]);
                msg.set_byte18(bytes[18]);
                msg.set_byte19(bytes[19]);
                msg.set_byte20(bytes[20]);
                msg.set_byte21(bytes[20]);
                msg.set_byte21(bytes[21]);
                msg.set_byte22(bytes[22]);
                msg.set_byte23(bytes[23]);
                msg.set_byte24(bytes[24]);
                msg.set_byte25(bytes[25]);
                msg.set_byte26(bytes[26]);
                msg.set_byte27(bytes[27]);
                msg.set_byte28(bytes[28]);
                msg.set_byte29(bytes[29]);
                msg.set_byte30(bytes[30]);
                msg.set_byte31(bytes[31]);
                msg.set_byte32(bytes[32]);
            }
            PublicKey::System => {
                let mut root = builder.init_root::<public_key_capnp::public_key::Builder>();
                root.set_system(());
            }
        }

        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)
            .map_err(|_| Error::UnableToSerialize)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for PublicKey {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .expect("unable to deserialize struct");

        let reader = deserialized
            .get_root::<public_key_capnp::public_key::Reader>()
            .map_err(|_| Error::UnableToDeserialize)?;
        match reader.which().map_err(|_| Error::UnableToDeserialize)? {
            public_key_capnp::public_key::Which::Ed25519(reader) => match reader {
                Ok(reader) => {
                    let bytes: [u8; PublicKey::ED25519_LENGTH] = [
                        reader.get_byte0(),
                        reader.get_byte1(),
                        reader.get_byte2(),
                        reader.get_byte3(),
                        reader.get_byte4(),
                        reader.get_byte5(),
                        reader.get_byte6(),
                        reader.get_byte7(),
                        reader.get_byte8(),
                        reader.get_byte9(),
                        reader.get_byte10(),
                        reader.get_byte11(),
                        reader.get_byte12(),
                        reader.get_byte13(),
                        reader.get_byte14(),
                        reader.get_byte15(),
                        reader.get_byte16(),
                        reader.get_byte17(),
                        reader.get_byte18(),
                        reader.get_byte19(),
                        reader.get_byte20(),
                        reader.get_byte21(),
                        reader.get_byte22(),
                        reader.get_byte23(),
                        reader.get_byte24(),
                        reader.get_byte25(),
                        reader.get_byte26(),
                        reader.get_byte27(),
                        reader.get_byte28(),
                        reader.get_byte29(),
                        reader.get_byte30(),
                        reader.get_byte31(),
                    ];

                    return Ok(PublicKey::ed25519_from_bytes(bytes)
                        .map_err(|_| Error::UnableToDeserialize)?);
                }
                Err(_) => return Err(Error::UnableToDeserialize),
            },
            public_key_capnp::public_key::Which::Secp256k1(reader) => match reader {
                Ok(reader) => {
                    let bytes: [u8; PublicKey::SECP256K1_LENGTH] = [
                        reader.get_byte0(),
                        reader.get_byte1(),
                        reader.get_byte2(),
                        reader.get_byte3(),
                        reader.get_byte4(),
                        reader.get_byte5(),
                        reader.get_byte6(),
                        reader.get_byte7(),
                        reader.get_byte8(),
                        reader.get_byte9(),
                        reader.get_byte10(),
                        reader.get_byte11(),
                        reader.get_byte12(),
                        reader.get_byte13(),
                        reader.get_byte14(),
                        reader.get_byte15(),
                        reader.get_byte16(),
                        reader.get_byte17(),
                        reader.get_byte18(),
                        reader.get_byte19(),
                        reader.get_byte20(),
                        reader.get_byte21(),
                        reader.get_byte22(),
                        reader.get_byte23(),
                        reader.get_byte24(),
                        reader.get_byte25(),
                        reader.get_byte26(),
                        reader.get_byte27(),
                        reader.get_byte28(),
                        reader.get_byte29(),
                        reader.get_byte30(),
                        reader.get_byte31(),
                        reader.get_byte32(),
                    ];
                    return Ok(PublicKey::secp256k1_from_bytes(bytes)
                        .map_err(|_| Error::UnableToDeserialize)?);
                }
                Err(_) => return Err(Error::UnableToDeserialize),
            },
            public_key_capnp::public_key::Which::System(_) => Ok(PublicKey::System),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        capnp::{FromCapnpBytes, ToCapnpBytes},
        PublicKey, SecretKey,
    };

    fn random_bytes(len: usize) -> Vec<u8> {
        let mut buf = vec![0; len];
        getrandom::getrandom(&mut buf).expect("should get random");
        buf
    }

    #[test]
    fn public_key_capnp_ed25519() {
        let _bytes = random_bytes(PublicKey::ED25519_LENGTH);
        let secret_key = SecretKey::ed25519_from_bytes([47; PublicKey::ED25519_LENGTH]) // TODO[RC]: Can't create from random bytes?
            .expect("should create secret key");

        let original: PublicKey = (&secret_key).into();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = PublicKey::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn public_key_capnp_secp256k1() {
        let random_bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key = SecretKey::secp256k1_from_bytes(random_bytes.as_slice())
            .expect("should create secret key");

        let original: PublicKey = (&secret_key).into();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = PublicKey::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn public_key_capnp_system() {
        let secret_key = SecretKey::System;

        let original: PublicKey = (&secret_key).into();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = PublicKey::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
