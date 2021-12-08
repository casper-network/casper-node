//! Generators for asymmetric key types

use core::convert::TryInto;

use proptest::{
    collection,
    prelude::{Arbitrary, Just, Strategy},
    prop_oneof,
};

use crate::{crypto::SecretKey, PublicKey};

/// Creates an arbitrary [`PublicKey`]
pub fn public_key_arb() -> impl Strategy<Value = PublicKey> {
    prop_oneof![
        Just(PublicKey::System),
        collection::vec(<u8>::arbitrary(), SecretKey::ED25519_LENGTH).prop_map(|bytes| {
            let byte_array: [u8; SecretKey::ED25519_LENGTH] = bytes.try_into().unwrap();
            let secret_key = SecretKey::ed25519_from_bytes(byte_array).unwrap();
            PublicKey::from(&secret_key)
        }),
        collection::vec(<u8>::arbitrary(), SecretKey::SECP256K1_LENGTH).prop_map(|bytes| {
            let bytes_array: [u8; SecretKey::SECP256K1_LENGTH] = bytes.try_into().unwrap();
            let secret_key = SecretKey::secp256k1_from_bytes(bytes_array).unwrap();
            PublicKey::from(&secret_key)
        })
    ]
}

/// Returns a strategy for creating random [`PublicKey`] instances but NOT system variant.
pub fn public_key_arb_no_system() -> impl Strategy<Value = PublicKey> {
    prop_oneof![
        collection::vec(<u8>::arbitrary(), SecretKey::ED25519_LENGTH).prop_map(|bytes| {
            let byte_array: [u8; SecretKey::ED25519_LENGTH] = bytes.try_into().unwrap();
            let secret_key = SecretKey::ed25519_from_bytes(byte_array).unwrap();
            PublicKey::from(&secret_key)
        }),
        collection::vec(<u8>::arbitrary(), SecretKey::SECP256K1_LENGTH).prop_map(|bytes| {
            let bytes_array: [u8; SecretKey::SECP256K1_LENGTH] = bytes.try_into().unwrap();
            let secret_key = SecretKey::secp256k1_from_bytes(bytes_array).unwrap();
            PublicKey::from(&secret_key)
        })
    ]
}
