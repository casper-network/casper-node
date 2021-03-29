//! Generators for asymmetric key types

use core::convert::TryInto;

use proptest::{
    collection,
    prelude::{Arbitrary, Just, Strategy},
    prop_oneof,
};

use crate::{crypto::SecretKey, AsymmetricType, PublicKey};

/// Creates an arbitrary [`SecretKey`]
pub fn secret_key_arb() -> impl Strategy<Value = SecretKey> {
    prop_oneof![
        Just(SecretKey::System),
        collection::vec(<u8>::arbitrary(), SecretKey::ED25519_LENGTH).prop_map(|bytes| {
            let byte_array: [u8; SecretKey::ED25519_LENGTH] = bytes.try_into().unwrap();
            SecretKey::ed25519_from_bytes(byte_array).unwrap()
        }),
        collection::vec(<u8>::arbitrary(), SecretKey::SECP256K1_LENGTH).prop_map(|bytes| {
            let bytes_array: [u8; SecretKey::SECP256K1_LENGTH] = bytes.try_into().unwrap();
            SecretKey::secp256k1_from_bytes(bytes_array).unwrap()
        })
    ]
}

/// Creates an arbitrary [`PublicKey`]
pub fn public_key_arb() -> impl Strategy<Value = PublicKey> {
    secret_key_arb().prop_map(Into::into)
}
