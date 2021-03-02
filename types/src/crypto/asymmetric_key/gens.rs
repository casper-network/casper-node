//! Generators for asymmetric key types

use core::convert::TryInto;

use proptest::{
    array, collection,
    prelude::{Arbitrary, Just, Strategy},
    prop_oneof,
};

use crate::{crypto::SecretKey, PublicKey};

/// Creates an arbitrary [`SecretKey`]
pub fn secret_key_arb() -> impl Strategy<Value = SecretKey> {
    prop_oneof![
        Just(SecretKey::System),
        array::uniform32(<u8>::arbitrary()).prop_map(SecretKey::ed25519),
        collection::vec(<u8>::arbitrary(), SecretKey::SECP256K1_LENGTH).prop_map(|bytes| {
            let bytes_array: [u8; SecretKey::SECP256K1_LENGTH] = bytes.try_into().unwrap();
            SecretKey::secp256k1(bytes_array)
        })
    ]
}

/// Creates an arbitrary [`PublicKey`]
pub fn public_key_arb() -> impl Strategy<Value = PublicKey> {
    secret_key_arb().prop_map(Into::into)
}
