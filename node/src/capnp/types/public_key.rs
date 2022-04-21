use casper_types::{AsymmetricType, PublicKey};

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};
use casper_node_macros::make_capnp_byte_setter_functions;

#[allow(dead_code)]
pub(super) mod public_key_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/public_key_capnp.rs"
    ));
}

// We cannot use the const literals directly
// const ED25519_PUBLIC_KEY_LENGTH = 32;
make_capnp_byte_setter_functions!(32, "ed25519", "public_key_capnp::ed25519_public_key");
// const SECP256K1_PUBLIC_KEY_LENGTH = 33;
make_capnp_byte_setter_functions!(33, "secp256k1", "public_key_capnp::secp256k1_public_key");

impl ToCapnpBuilder<PublicKey> for public_key_capnp::public_key::Builder<'_> {
    fn try_to_builder(&mut self, public_key: &PublicKey) -> Result<(), SerializeError> {
        match public_key {
            PublicKey::Ed25519(key) => {
                let bytes = key.as_bytes();
                let mut msg = self.reborrow().init_ed25519();
                set_ed25519(&mut msg, bytes);
            }
            PublicKey::Secp256k1(key) => {
                let bytes = key.to_bytes();
                let mut msg = self.reborrow().init_secp256k1();
                set_secp256k1(&mut msg, &bytes);
            }
            PublicKey::System => {
                self.set_system(());
            }
        }
        Ok(())
    }
}

impl FromCapnpReader<PublicKey> for public_key_capnp::public_key::Reader<'_> {
    fn try_from_reader(&self) -> Result<PublicKey, DeserializeError> {
        match self.which()? {
            public_key_capnp::public_key::Which::Ed25519(reader) => match reader {
                Ok(reader) => {
                    let bytes: [u8; PublicKey::ED25519_LENGTH] = get_ed25519(reader);
                    PublicKey::ed25519_from_bytes(bytes).map_err(DeserializeError::from)
                }
                Err(e) => Err(e.into()),
            },
            public_key_capnp::public_key::Which::Secp256k1(reader) => match reader {
                Ok(reader) => {
                    let bytes: [u8; PublicKey::SECP256K1_LENGTH] = get_secp256k1(reader);
                    PublicKey::secp256k1_from_bytes(bytes).map_err(DeserializeError::from)
                }
                Err(e) => Err(e.into()),
            },
            public_key_capnp::public_key::Which::System(_) => Ok(PublicKey::System),
        }
    }
}

impl ToCapnpBytes for PublicKey {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<public_key_capnp::public_key::Builder>();
        msg.try_to_builder(self)?;

        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for PublicKey {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .expect("unable to deserialize struct");

        let reader = deserialized.get_root::<public_key_capnp::public_key::Reader>()?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use casper_types::{PublicKey, SecretKey};

    use super::super::random_bytes;
    use crate::capnp::{FromCapnpBytes, ToCapnpBytes};

    pub(crate) fn random_key_pair() -> (PublicKey, SecretKey) {
        let secret_key = match random_bytes(1)[0] {
            b if b % 3 == 0 => {
                let _bytes = random_bytes(PublicKey::ED25519_LENGTH);
                // TODO[RC]: Can't create from random bytes?
                SecretKey::ed25519_from_bytes([47; PublicKey::ED25519_LENGTH])
                    .expect("should create secret key")
            }
            b if b % 3 == 1 => {
                let random_bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
                SecretKey::secp256k1_from_bytes(random_bytes.as_slice())
                    .expect("should create secret key")
            }
            b if b % 3 == 2 => SecretKey::System,
            _ => unreachable!(),
        };
        ((&secret_key).into(), secret_key)
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

    #[test]
    fn public_key_capnp() {
        let (public_key, _) = random_key_pair();

        let original = public_key;
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = PublicKey::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
