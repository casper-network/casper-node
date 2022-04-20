use casper_types::{AsymmetricType, PublicKey};

use casper_node_macros::{make_ed25519_capnp_functions, make_secp256k1_capnp_functions};

use crate::capnp::{Error, FromCapnpBytes, ToCapnpBytes};

#[allow(dead_code)]
pub(super) mod public_key_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/public_key_capnp.rs"
    ));
}

make_ed25519_capnp_functions!();
make_secp256k1_capnp_functions!();

pub(crate) fn put_public_key_into_builder(
    public_key: &PublicKey,
    builder: &mut public_key_capnp::public_key::Builder<'_>,
) {
    match public_key {
        PublicKey::Ed25519(key) => {
            let bytes = key.as_bytes();
            let mut msg = builder.reborrow().init_ed25519();
            set_ed25519(&mut msg, bytes);
        }
        PublicKey::Secp256k1(key) => {
            let bytes = key.to_bytes();
            let mut msg = builder.reborrow().init_secp256k1();
            set_secp256k1(&mut msg, &bytes);
        }
        PublicKey::System => {
            builder.set_system(());
        }
    }
}

impl ToCapnpBytes for PublicKey {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut builder = capnp::message::Builder::new_default();
        let mut public_key_builder = builder.init_root::<public_key_capnp::public_key::Builder>();
        put_public_key_into_builder(&self, &mut public_key_builder);

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
                    let bytes: [u8; PublicKey::ED25519_LENGTH] = get_ed25519(reader);
                    return Ok(PublicKey::ed25519_from_bytes(bytes)
                        .map_err(|_| Error::UnableToDeserialize)?);
                }
                Err(_) => return Err(Error::UnableToDeserialize),
            },
            public_key_capnp::public_key::Which::Secp256k1(reader) => match reader {
                Ok(reader) => {
                    let bytes: [u8; PublicKey::SECP256K1_LENGTH] = get_secp256k1(reader);
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
    use casper_types::{PublicKey, SecretKey};

    use crate::capnp::{FromCapnpBytes, ToCapnpBytes};

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
