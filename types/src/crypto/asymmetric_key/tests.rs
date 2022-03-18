use capnp_conv::{FromCapnpBytes, ToCapnpBytes};
use proptest_attr_macro::proptest;

use crate::{crypto::SecretKey, PublicKey};

#[test]
fn can_construct_ed25519_keypair_from_zeroes() {
    let bytes = [0; SecretKey::ED25519_LENGTH];
    let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
    let _public_key: PublicKey = (&secret_key).into();
}

#[test]
#[should_panic]
fn cannot_construct_secp256k1_keypair_from_zeroes() {
    let bytes = [0; SecretKey::SECP256K1_LENGTH];
    let secret_key = SecretKey::secp256k1_from_bytes(bytes).unwrap();
    let _public_key: PublicKey = (&secret_key).into();
}

#[test]
fn can_construct_ed25519_keypair_from_ones() {
    let bytes = [1; SecretKey::ED25519_LENGTH];
    let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
    let _public_key: PublicKey = (&secret_key).into();
}

#[test]
fn can_construct_secp256k1_keypair_from_ones() {
    let bytes = [1; SecretKey::SECP256K1_LENGTH];
    let secret_key = SecretKey::secp256k1_from_bytes(bytes).unwrap();
    let _public_key: PublicKey = (&secret_key).into();
}

// Capnproto tests

#[test]
fn system_contract_public_key_capnp_roundtrip() {
    let system_contract_public_key = PublicKey::System;
    assert_eq!(
        system_contract_public_key,
        PublicKey::from_packed_capnp_bytes(&system_contract_public_key.to_packed_capnp_bytes())
            .unwrap()
    )
}

#[proptest]
fn ed25519_pubkey_capnp_roundtrip(bytes: [u8; SecretKey::ED25519_LENGTH]) {
    let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
    let public_key: PublicKey = (&secret_key).into();
    assert_eq!(
        public_key,
        PublicKey::from_packed_capnp_bytes(&public_key.to_packed_capnp_bytes()).unwrap()
    )
}

#[proptest]
fn secp256k1_pubkey_capnp_roundtrip(bytes: [u8; SecretKey::SECP256K1_LENGTH]) {
    // Zeros aren't allowed as a secret key...
    let bytes = if bytes == [0; SecretKey::SECP256K1_LENGTH] {
        [1; SecretKey::SECP256K1_LENGTH]
    } else {
        bytes
    };
    let secret_key = SecretKey::secp256k1_from_bytes(bytes).unwrap();
    let public_key: PublicKey = (&secret_key).into();
    assert_eq!(
        public_key,
        PublicKey::from_packed_capnp_bytes(&public_key.to_packed_capnp_bytes()).unwrap()
    )
}
