use crate::{crypto::SecretKey, PublicKey};

#[test]
fn can_construct_ed25519_keypair_from_zeroes() {
    let bytes = [0; SecretKey::ED25519_LENGTH];
    let secret_key = SecretKey::ed25519(bytes);
    let _public_key: PublicKey = secret_key.into();
}

#[test]
#[should_panic]
fn cannot_construct_secp256k1_keypair_from_zeroes() {
    let bytes = [0; SecretKey::SECP256K1_LENGTH];
    let secret_key = SecretKey::secp256k1(bytes);
    let _public_key: PublicKey = secret_key.into();
}

#[test]
fn can_construct_ed25519_keypair_from_ones() {
    let bytes = [1; SecretKey::ED25519_LENGTH];
    let secret_key = SecretKey::ed25519(bytes);
    let _public_key: PublicKey = secret_key.into();
}

#[test]
fn can_construct_secp256k1_keypair_from_ones() {
    let bytes = [1; SecretKey::SECP256K1_LENGTH];
    let secret_key = SecretKey::secp256k1(bytes);
    let _public_key: PublicKey = secret_key.into();
}

#[test]
fn can_construct_system_public_key() {
    let public_key_bytes = [0; PublicKey::ED25519_LENGTH];
    let public_key = PublicKey::ed25519(public_key_bytes).unwrap();

    let secret_key_bytes = [0; SecretKey::ED25519_LENGTH];
    let secret_key = SecretKey::ed25519(secret_key_bytes);

    assert_ne!(public_key, secret_key.into())
}
