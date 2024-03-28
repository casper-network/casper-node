use std::{
    cmp::Ordering,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    iter,
};

use rand::RngCore;

use k256::elliptic_curve::sec1::ToEncodedPoint;
use openssl::pkey::{PKey, Private, Public};

use super::*;
use crate::{
    bytesrepr, checksummed_hex, crypto::SecretKey, testing::TestRng, AsymmetricType, PublicKey,
    Tagged,
};

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

type OpenSSLSecretKey = PKey<Private>;
type OpenSSLPublicKey = PKey<Public>;

// `SecretKey` does not implement `PartialEq`, so just compare derived `PublicKey`s.
fn assert_secret_keys_equal(lhs: &SecretKey, rhs: &SecretKey) {
    assert_eq!(PublicKey::from(lhs), PublicKey::from(rhs));
}

fn secret_key_der_roundtrip(secret_key: SecretKey) {
    let der_encoded = secret_key.to_der().unwrap();
    let decoded = SecretKey::from_der(&der_encoded).unwrap();
    assert_secret_keys_equal(&secret_key, &decoded);
    assert_eq!(secret_key.tag(), decoded.tag());

    // Ensure malformed encoded version fails to decode.
    SecretKey::from_der(&der_encoded[1..]).unwrap_err();
}

fn secret_key_pem_roundtrip(secret_key: SecretKey) {
    let pem_encoded = secret_key.to_pem().unwrap();
    let decoded = SecretKey::from_pem(pem_encoded.as_bytes()).unwrap();
    assert_secret_keys_equal(&secret_key, &decoded);
    assert_eq!(secret_key.tag(), decoded.tag());

    // Check PEM-encoded can be decoded by openssl.
    let _ = OpenSSLSecretKey::private_key_from_pem(pem_encoded.as_bytes()).unwrap();

    // Ensure malformed encoded version fails to decode.
    SecretKey::from_pem(&pem_encoded[1..]).unwrap_err();
}

fn known_secret_key_to_pem(expected_key: &SecretKey, known_key_pem: &str, expected_tag: u8) {
    let decoded = SecretKey::from_pem(known_key_pem.as_bytes()).unwrap();
    assert_secret_keys_equal(expected_key, &decoded);
    assert_eq!(expected_tag, decoded.tag());
}

fn secret_key_file_roundtrip(secret_key: SecretKey) {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("test_secret_key.pem");

    secret_key.to_file(&path).unwrap();
    let decoded = SecretKey::from_file(&path).unwrap();
    assert_secret_keys_equal(&secret_key, &decoded);
    assert_eq!(secret_key.tag(), decoded.tag());
}

fn public_key_serialization_roundtrip(public_key: PublicKey) {
    // Try to/from bincode.
    let serialized = bincode::serialize(&public_key).unwrap();
    let deserialized = bincode::deserialize(&serialized).unwrap();
    assert_eq!(public_key, deserialized);
    assert_eq!(public_key.tag(), deserialized.tag());

    // Try to/from JSON.
    let serialized = serde_json::to_vec_pretty(&public_key).unwrap();
    let deserialized = serde_json::from_slice(&serialized).unwrap();
    assert_eq!(public_key, deserialized);
    assert_eq!(public_key.tag(), deserialized.tag());

    // Using bytesrepr.
    bytesrepr::test_serialization_roundtrip(&public_key);
}

fn public_key_der_roundtrip(public_key: PublicKey) {
    let der_encoded = public_key.to_der().unwrap();
    let decoded = PublicKey::from_der(&der_encoded).unwrap();
    assert_eq!(public_key, decoded);

    // Check DER-encoded can be decoded by openssl.
    let _ = OpenSSLPublicKey::public_key_from_der(&der_encoded).unwrap();

    // Ensure malformed encoded version fails to decode.
    PublicKey::from_der(&der_encoded[1..]).unwrap_err();
}

fn public_key_pem_roundtrip(public_key: PublicKey) {
    let pem_encoded = public_key.to_pem().unwrap();
    let decoded = PublicKey::from_pem(pem_encoded.as_bytes()).unwrap();
    assert_eq!(public_key, decoded);
    assert_eq!(public_key.tag(), decoded.tag());

    // Check PEM-encoded can be decoded by openssl.
    let _ = OpenSSLPublicKey::public_key_from_pem(pem_encoded.as_bytes()).unwrap();

    // Ensure malformed encoded version fails to decode.
    PublicKey::from_pem(&pem_encoded[1..]).unwrap_err();
}

fn known_public_key_to_pem(known_key_hex: &str, known_key_pem: &str) {
    let key_bytes = checksummed_hex::decode(known_key_hex).unwrap();
    let decoded = PublicKey::from_pem(known_key_pem.as_bytes()).unwrap();
    assert_eq!(key_bytes, Into::<Vec<u8>>::into(decoded));
}

fn public_key_file_roundtrip(public_key: PublicKey) {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("test_public_key.pem");

    public_key.to_file(&path).unwrap();
    let decoded = PublicKey::from_file(&path).unwrap();
    assert_eq!(public_key, decoded);
}

fn public_key_hex_roundtrip(public_key: PublicKey) {
    let hex_encoded = public_key.to_hex();
    let decoded = PublicKey::from_hex(&hex_encoded).unwrap();
    assert_eq!(public_key, decoded);
    assert_eq!(public_key.tag(), decoded.tag());

    // Ensure malformed encoded version fails to decode.
    PublicKey::from_hex(&hex_encoded[..1]).unwrap_err();
    PublicKey::from_hex(&hex_encoded[1..]).unwrap_err();
}

fn signature_serialization_roundtrip(signature: Signature) {
    // Try to/from bincode.
    let serialized = bincode::serialize(&signature).unwrap();
    let deserialized: Signature = bincode::deserialize(&serialized).unwrap();
    assert_eq!(signature, deserialized);
    assert_eq!(signature.tag(), deserialized.tag());

    // Try to/from JSON.
    let serialized = serde_json::to_vec_pretty(&signature).unwrap();
    let deserialized = serde_json::from_slice(&serialized).unwrap();
    assert_eq!(signature, deserialized);
    assert_eq!(signature.tag(), deserialized.tag());

    // Try to/from using bytesrepr.
    let serialized = bytesrepr::serialize(signature).unwrap();
    let deserialized = bytesrepr::deserialize(serialized).unwrap();
    assert_eq!(signature, deserialized);
    assert_eq!(signature.tag(), deserialized.tag())
}

fn signature_hex_roundtrip(signature: Signature) {
    let hex_encoded = signature.to_hex();
    let decoded = Signature::from_hex(hex_encoded.as_bytes()).unwrap();
    assert_eq!(signature, decoded);
    assert_eq!(signature.tag(), decoded.tag());

    // Ensure malformed encoded version fails to decode.
    Signature::from_hex(&hex_encoded[..1]).unwrap_err();
    Signature::from_hex(&hex_encoded[1..]).unwrap_err();
}

fn hash<T: Hash>(data: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

fn check_ord_and_hash<T: Clone + Ord + PartialOrd + Hash>(low: T, high: T) {
    #[allow(clippy::redundant_clone)]
    let low_copy = low.clone();

    assert_eq!(hash(&low), hash(&low_copy));
    assert_ne!(hash(&low), hash(&high));

    assert_eq!(Ordering::Less, low.cmp(&high));
    assert_eq!(Some(Ordering::Less), low.partial_cmp(&high));

    assert_eq!(Ordering::Greater, high.cmp(&low));
    assert_eq!(Some(Ordering::Greater), high.partial_cmp(&low));

    assert_eq!(Ordering::Equal, low.cmp(&low_copy));
    assert_eq!(Some(Ordering::Equal), low.partial_cmp(&low_copy));
}

mod system {
    use std::path::Path;

    use super::{sign, verify};
    use crate::crypto::{AsymmetricType, PublicKey, SecretKey, Signature};

    #[test]
    fn secret_key_to_der_should_error() {
        assert!(SecretKey::system().to_der().is_err());
    }

    #[test]
    fn secret_key_to_pem_should_error() {
        assert!(SecretKey::system().to_pem().is_err());
    }

    #[test]
    fn secret_key_to_file_should_error() {
        assert!(SecretKey::system().to_file(Path::new("/dev/null")).is_err());
    }

    #[test]
    fn public_key_serialization_roundtrip() {
        super::public_key_serialization_roundtrip(PublicKey::system());
    }

    #[test]
    fn public_key_to_der_should_error() {
        assert!(PublicKey::system().to_der().is_err());
    }

    #[test]
    fn public_key_to_pem_should_error() {
        assert!(PublicKey::system().to_pem().is_err());
    }

    #[test]
    fn public_key_to_file_should_error() {
        assert!(PublicKey::system().to_file(Path::new("/dev/null")).is_err());
    }

    #[test]
    fn public_key_to_and_from_hex() {
        super::public_key_hex_roundtrip(PublicKey::system());
    }

    #[test]
    #[should_panic]
    fn sign_should_panic() {
        sign([], &SecretKey::system(), &PublicKey::system());
    }

    #[test]
    fn signature_to_and_from_hex() {
        super::signature_hex_roundtrip(Signature::system());
    }

    #[test]
    fn public_key_to_account_hash() {
        assert_ne!(
            PublicKey::system().to_account_hash().as_ref(),
            Into::<Vec<u8>>::into(PublicKey::system())
        );
    }

    #[test]
    fn verify_should_error() {
        assert!(verify([], &Signature::system(), &PublicKey::system()).is_err());
    }

    #[test]
    fn bytesrepr_roundtrip_signature() {
        crate::bytesrepr::test_serialization_roundtrip(&Signature::system());
    }
}

mod ed25519 {
    use rand::Rng;

    use super::*;
    use crate::ED25519_TAG;

    const SECRET_KEY_LENGTH: usize = SecretKey::ED25519_LENGTH;
    const PUBLIC_KEY_LENGTH: usize = PublicKey::ED25519_LENGTH;
    const SIGNATURE_LENGTH: usize = Signature::ED25519_LENGTH;

    #[test]
    fn secret_key_from_bytes() {
        // Secret key should be `SecretKey::ED25519_LENGTH` bytes.
        let bytes = [0; SECRET_KEY_LENGTH + 1];
        assert!(SecretKey::ed25519_from_bytes(&bytes[..]).is_err());
        assert!(SecretKey::ed25519_from_bytes(&bytes[2..]).is_err());

        // Check the same bytes but of the right length succeeds.
        assert!(SecretKey::ed25519_from_bytes(&bytes[1..]).is_ok());
    }

    #[test]
    fn secret_key_to_and_from_der() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_ed25519(&mut rng);
        let der_encoded = secret_key.to_der().unwrap();
        secret_key_der_roundtrip(secret_key);

        // Check DER-encoded can be decoded by openssl.
        let _ = OpenSSLSecretKey::private_key_from_der(&der_encoded).unwrap();
    }

    #[test]
    fn secret_key_to_and_from_pem() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_ed25519(&mut rng);
        secret_key_pem_roundtrip(secret_key);
    }

    #[test]
    fn known_secret_key_to_pem() {
        // Example values taken from https://tools.ietf.org/html/rfc8410#section-10.3
        const KNOWN_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----"#;
        let key_bytes =
            base16::decode("d4ee72dbf913584ad5b6d8f1f769f8ad3afe7c28cbf1d4fbe097a88f44755842")
                .unwrap();
        let expected_key = SecretKey::ed25519_from_bytes(key_bytes).unwrap();
        super::known_secret_key_to_pem(&expected_key, KNOWN_KEY_PEM, ED25519_TAG);
    }

    #[test]
    fn secret_key_to_and_from_file() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_ed25519(&mut rng);
        secret_key_file_roundtrip(secret_key);
    }

    #[test]
    fn public_key_serialization_roundtrip() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_ed25519(&mut rng);
        super::public_key_serialization_roundtrip(public_key);
    }

    #[test]
    fn public_key_from_bytes() {
        // Public key should be `PublicKey::ED25519_LENGTH` bytes.  Create vec with an extra
        // byte.
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_ed25519(&mut rng);
        let bytes: Vec<u8> = iter::once(rng.gen())
            .chain(Into::<Vec<u8>>::into(public_key))
            .collect::<Vec<u8>>();

        assert!(PublicKey::ed25519_from_bytes(&bytes[..]).is_err());
        assert!(PublicKey::ed25519_from_bytes(&bytes[2..]).is_err());

        // Check the same bytes but of the right length succeeds.
        assert!(PublicKey::ed25519_from_bytes(&bytes[1..]).is_ok());
    }

    #[test]
    fn public_key_to_and_from_der() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_ed25519(&mut rng);
        public_key_der_roundtrip(public_key);
    }

    #[test]
    fn public_key_to_and_from_pem() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_ed25519(&mut rng);
        public_key_pem_roundtrip(public_key);
    }

    #[test]
    fn known_public_key_to_pem() {
        // Example values taken from https://tools.ietf.org/html/rfc8410#section-10.1
        const KNOWN_KEY_HEX: &str =
            "19bf44096984cdfe8541bac167dc3b96c85086aa30b6b6cb0c5c38ad703166e1";
        const KNOWN_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAGb9ECWmEzf6FQbrBZ9w7lshQhqowtrbLDFw4rXAxZuE=
-----END PUBLIC KEY-----"#;
        super::known_public_key_to_pem(KNOWN_KEY_HEX, KNOWN_KEY_PEM);
    }

    #[test]
    fn public_key_to_and_from_file() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_ed25519(&mut rng);
        public_key_file_roundtrip(public_key);
    }

    #[test]
    fn public_key_to_and_from_hex() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_ed25519(&mut rng);
        public_key_hex_roundtrip(public_key);
    }

    #[test]
    fn signature_serialization_roundtrip() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_ed25519(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        let data = b"data";
        let signature = sign(data, &secret_key, &public_key);
        super::signature_serialization_roundtrip(signature);
    }

    #[test]
    fn signature_from_bytes() {
        // Signature should be `Signature::ED25519_LENGTH` bytes.
        let bytes = [2; SIGNATURE_LENGTH + 1];
        assert!(Signature::ed25519_from_bytes(&bytes[..]).is_err());
        assert!(Signature::ed25519_from_bytes(&bytes[2..]).is_err());

        // Check the same bytes but of the right length succeeds.
        assert!(Signature::ed25519_from_bytes(&bytes[1..]).is_ok());
    }

    #[test]
    fn signature_key_to_and_from_hex() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_ed25519(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        let data = b"data";
        let signature = sign(data, &secret_key, &public_key);
        signature_hex_roundtrip(signature);
    }

    #[test]
    fn public_key_traits() {
        let public_key_low = PublicKey::ed25519_from_bytes([1; PUBLIC_KEY_LENGTH]).unwrap();
        let public_key_high = PublicKey::ed25519_from_bytes([3; PUBLIC_KEY_LENGTH]).unwrap();
        check_ord_and_hash(public_key_low, public_key_high)
    }

    #[test]
    fn public_key_to_account_hash() {
        let public_key_high = PublicKey::ed25519_from_bytes([255; PUBLIC_KEY_LENGTH]).unwrap();
        assert_ne!(
            public_key_high.to_account_hash().as_ref(),
            Into::<Vec<u8>>::into(public_key_high)
        );
    }

    #[test]
    fn signature_traits() {
        let signature_low = Signature::ed25519([1; SIGNATURE_LENGTH]).unwrap();
        let signature_high = Signature::ed25519([3; SIGNATURE_LENGTH]).unwrap();
        check_ord_and_hash(signature_low, signature_high)
    }

    #[test]
    fn sign_and_verify() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_ed25519(&mut rng);

        let public_key = PublicKey::from(&secret_key);
        let other_public_key = PublicKey::random_ed25519(&mut rng);
        let wrong_type_public_key = PublicKey::random_secp256k1(&mut rng);

        let message = b"message";
        let signature = sign(message, &secret_key, &public_key);

        assert!(verify(message, &signature, &public_key).is_ok());
        assert!(verify(message, &signature, &other_public_key).is_err());
        assert!(verify(message, &signature, &wrong_type_public_key).is_err());
        assert!(verify(&message[1..], &signature, &public_key).is_err());
    }

    #[test]
    fn bytesrepr_roundtrip_signature() {
        let mut rng = TestRng::new();
        let ed25519_secret_key = SecretKey::random_ed25519(&mut rng);
        let public_key = PublicKey::from(&ed25519_secret_key);
        let data = b"data";
        let signature = sign(data, &ed25519_secret_key, &public_key);
        bytesrepr::test_serialization_roundtrip(&signature);
    }

    #[test]
    fn validate_known_signature() {
        // In the event that this test fails, we need to consider pinning the version of the
        // `ed25519-dalek` crate to maintain backwards compatibility with existing data on the
        // Casper network.

        // Values taken from:
        // https://github.com/dalek-cryptography/ed25519-dalek/blob/925eb9ea56192053c9eb93b9d30d1b9419eee128/TESTVECTORS#L62
        let secret_key_hex = "bf5ba5d6a49dd5ef7b4d5d7d3e4ecc505c01f6ccee4c54b5ef7b40af6a454140";
        let public_key_hex = "1be034f813017b900d8990af45fad5b5214b573bd303ef7a75ef4b8c5c5b9842";
        let message_hex =
            "16152c2e037b1c0d3219ced8e0674aee6b57834b55106c5344625322da638ecea2fc9a424a05ee9512\
                d48fcf75dd8bd4691b3c10c28ec98ee1afa5b863d1c36795ed18105db3a9aabd9d2b4c1747adbaf1a56\
                ffcc0c533c1c0faef331cdb79d961fa39f880a1b8b1164741822efb15a7259a465bef212855751fab66\
                a897bfa211abe0ea2f2e1cd8a11d80e142cde1263eec267a3138ae1fcf4099db0ab53d64f336f4bcd7a\
                363f6db112c0a2453051a0006f813aaf4ae948a2090619374fa58052409c28ef76225687df3cb2d1b0b\
                fb43b09f47f1232f790e6d8dea759e57942099f4c4bd3390f28afc2098244961465c643fc8b29766af2\
                bcbc5440b86e83608cfc937be98bb4827fd5e6b689adc2e26513db531076a6564396255a09975b7034d\
                ac06461b255642e3a7ed75fa9fc265011f5f6250382a84ac268d63ba64";
        let signature_hex =
            "279cace6fdaf3945e3837df474b28646143747632bede93e7a66f5ca291d2c24978512ca0cb8827c8c\
                322685bd605503a5ec94dbae61bbdcae1e49650602bc07";

        let secret_key_bytes = base16::decode(secret_key_hex).unwrap();
        let public_key_bytes = base16::decode(public_key_hex).unwrap();
        let message_bytes = base16::decode(message_hex).unwrap();
        let signature_bytes = base16::decode(signature_hex).unwrap();

        let secret_key = SecretKey::ed25519_from_bytes(secret_key_bytes).unwrap();
        let public_key = PublicKey::ed25519_from_bytes(public_key_bytes).unwrap();
        assert_eq!(public_key, PublicKey::from(&secret_key));

        let signature = Signature::ed25519_from_bytes(signature_bytes).unwrap();
        assert_eq!(sign(&message_bytes, &secret_key, &public_key), signature);
        assert!(verify(&message_bytes, &signature, &public_key).is_ok());
    }
}

mod secp256k1 {
    use rand::Rng;

    use super::*;
    use crate::SECP256K1_TAG;

    const SECRET_KEY_LENGTH: usize = SecretKey::SECP256K1_LENGTH;
    const SIGNATURE_LENGTH: usize = Signature::SECP256K1_LENGTH;

    #[test]
    fn secret_key_from_bytes() {
        // Secret key should be `SecretKey::SECP256K1_LENGTH` bytes.
        // The k256 library will ensure that a byte stream of a length not equal to
        // `SECP256K1_LENGTH` will fail due to an assertion internal to the library.
        // We can check that invalid byte streams e.g [0;32] does not generate a valid key.
        let bytes = [0; SECRET_KEY_LENGTH];
        assert!(SecretKey::secp256k1_from_bytes(&bytes[..]).is_err());

        // Check that a valid byte stream produces a valid key
        let bytes = [1; SECRET_KEY_LENGTH];
        assert!(SecretKey::secp256k1_from_bytes(&bytes[..]).is_ok());
    }

    #[test]
    fn secret_key_to_and_from_der() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_secp256k1(&mut rng);
        secret_key_der_roundtrip(secret_key);
    }

    #[test]
    fn secret_key_to_and_from_pem() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_secp256k1(&mut rng);
        secret_key_pem_roundtrip(secret_key);
    }

    #[test]
    fn known_secret_key_to_pem() {
        // Example values taken from Python client.
        const KNOWN_KEY_PEM: &str = r#"-----BEGIN EC PRIVATE KEY-----
MHQCAQEEIL3fqaMKAfXSK1D2PnVVbZlZ7jTv133nukq4+95s6kmcoAcGBSuBBAAK
oUQDQgAEQI6VJjFv0fje9IDdRbLMcv/XMnccnOtdkv+kBR5u4ISEAkuc2TFWQHX0
Yj9oTB9fx9+vvQdxJOhMtu46kGo0Uw==
-----END EC PRIVATE KEY-----"#;
        let key_bytes =
            base16::decode("bddfa9a30a01f5d22b50f63e75556d9959ee34efd77de7ba4ab8fbde6cea499c")
                .unwrap();
        let expected_key = SecretKey::secp256k1_from_bytes(key_bytes).unwrap();
        super::known_secret_key_to_pem(&expected_key, KNOWN_KEY_PEM, SECP256K1_TAG);
    }

    #[test]
    fn secret_key_to_and_from_file() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_secp256k1(&mut rng);
        secret_key_file_roundtrip(secret_key);
    }

    #[test]
    fn public_key_serialization_roundtrip() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        super::public_key_serialization_roundtrip(public_key);
    }

    #[test]
    fn public_key_from_bytes() {
        // Public key should be `PublicKey::SECP256K1_LENGTH` bytes.  Create vec with an extra
        // byte.
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        let bytes: Vec<u8> = iter::once(rng.gen())
            .chain(Into::<Vec<u8>>::into(public_key))
            .collect::<Vec<u8>>();

        assert!(PublicKey::secp256k1_from_bytes(&bytes[..]).is_err());
        assert!(PublicKey::secp256k1_from_bytes(&bytes[2..]).is_err());

        // Check the same bytes but of the right length succeeds.
        assert!(PublicKey::secp256k1_from_bytes(&bytes[1..]).is_ok());
    }

    #[test]
    fn public_key_to_and_from_der() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        public_key_der_roundtrip(public_key);
    }

    #[test]
    fn public_key_to_and_from_pem() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        public_key_pem_roundtrip(public_key);
    }

    #[test]
    fn known_public_key_to_pem() {
        // Example values taken from Python client.
        const KNOWN_KEY_HEX: &str =
            "03408e9526316fd1f8def480dd45b2cc72ffd732771c9ceb5d92ffa4051e6ee084";
        const KNOWN_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEQI6VJjFv0fje9IDdRbLMcv/XMnccnOtd
kv+kBR5u4ISEAkuc2TFWQHX0Yj9oTB9fx9+vvQdxJOhMtu46kGo0Uw==
-----END PUBLIC KEY-----"#;
        super::known_public_key_to_pem(KNOWN_KEY_HEX, KNOWN_KEY_PEM);
    }

    #[test]
    fn public_key_to_and_from_file() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        public_key_file_roundtrip(public_key);
    }

    #[test]
    fn public_key_to_and_from_hex() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        public_key_hex_roundtrip(public_key);
    }

    #[test]
    fn signature_serialization_roundtrip() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_secp256k1(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        let data = b"data";
        let signature = sign(data, &secret_key, &public_key);
        super::signature_serialization_roundtrip(signature);
    }

    #[test]
    fn bytesrepr_roundtrip_signature() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_secp256k1(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        let data = b"data";
        let signature = sign(data, &secret_key, &public_key);
        bytesrepr::test_serialization_roundtrip(&signature);
    }

    #[test]
    fn signature_from_bytes() {
        // Signature should be `Signature::SECP256K1_LENGTH` bytes.
        let bytes = [2; SIGNATURE_LENGTH + 1];
        assert!(Signature::secp256k1_from_bytes(&bytes[..]).is_err());
        assert!(Signature::secp256k1_from_bytes(&bytes[2..]).is_err());

        // Check the same bytes but of the right length succeeds.
        assert!(Signature::secp256k1_from_bytes(&bytes[1..]).is_ok());
    }

    #[test]
    fn signature_key_to_and_from_hex() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random_secp256k1(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        let data = b"data";
        let signature = sign(data, &secret_key, &public_key);
        signature_hex_roundtrip(signature);
    }

    #[test]
    fn public_key_traits() {
        let mut rng = TestRng::new();
        let public_key1 = PublicKey::random_secp256k1(&mut rng);
        let public_key2 = PublicKey::random_secp256k1(&mut rng);
        if Into::<Vec<u8>>::into(public_key1.clone()) < Into::<Vec<u8>>::into(public_key2.clone()) {
            check_ord_and_hash(public_key1, public_key2)
        } else {
            check_ord_and_hash(public_key2, public_key1)
        }
    }

    #[test]
    fn public_key_to_account_hash() {
        let mut rng = TestRng::new();
        let public_key = PublicKey::random_secp256k1(&mut rng);
        assert_ne!(
            public_key.to_account_hash().as_ref(),
            Into::<Vec<u8>>::into(public_key)
        );
    }

    #[test]
    fn signature_traits() {
        let signature_low = Signature::secp256k1([1; SIGNATURE_LENGTH]).unwrap();
        let signature_high = Signature::secp256k1([3; SIGNATURE_LENGTH]).unwrap();
        check_ord_and_hash(signature_low, signature_high)
    }

    #[test]
    fn validate_known_signature() {
        // In the event that this test fails, we need to consider pinning the version of the
        // `k256` crate to maintain backwards compatibility with existing data on the Casper
        // network.
        let secret_key_hex = "833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42";
        let public_key_hex = "028e24fd9654f12c793d3d376c15f7abe53e0fbd537884a3a98d10d2dc6d513b4e";
        let message_hex = "616263";
        let signature_hex = "8016162860f0795154643d15c5ab5bb840d8c695d6de027421755579ea7f2a4629b7e0c88fc3428669a6a89496f426181b73f10c6c8a05ac8f49d6cb5032eb89";

        let secret_key_bytes = base16::decode(secret_key_hex).unwrap();
        let public_key_bytes = base16::decode(public_key_hex).unwrap();
        let message_bytes = base16::decode(message_hex).unwrap();
        let signature_bytes = base16::decode(signature_hex).unwrap();

        let secret_key = SecretKey::secp256k1_from_bytes(secret_key_bytes).unwrap();
        let public_key = PublicKey::secp256k1_from_bytes(public_key_bytes).unwrap();
        assert_eq!(public_key, PublicKey::from(&secret_key));

        let signature = Signature::secp256k1_from_bytes(signature_bytes).unwrap();
        assert_eq!(sign(&message_bytes, &secret_key, &public_key), signature);
        assert!(verify(&message_bytes, &signature, &public_key).is_ok());
    }
}

#[test]
fn public_key_traits() {
    let system_key = PublicKey::system();
    let mut rng = TestRng::new();
    let ed25519_public_key = PublicKey::random_ed25519(&mut rng);
    let secp256k1_public_key = PublicKey::random_secp256k1(&mut rng);
    check_ord_and_hash(ed25519_public_key.clone(), secp256k1_public_key.clone());
    check_ord_and_hash(system_key.clone(), ed25519_public_key);
    check_ord_and_hash(system_key, secp256k1_public_key);
}

#[test]
fn signature_traits() {
    let system_sig = Signature::system();
    let ed25519_sig = Signature::ed25519([3; Signature::ED25519_LENGTH]).unwrap();
    let secp256k1_sig = Signature::secp256k1([1; Signature::SECP256K1_LENGTH]).unwrap();
    check_ord_and_hash(ed25519_sig, secp256k1_sig);
    check_ord_and_hash(system_sig, ed25519_sig);
    check_ord_and_hash(system_sig, secp256k1_sig);
}

#[test]
fn sign_and_verify() {
    let mut rng = TestRng::new();
    let ed25519_secret_key = SecretKey::random_ed25519(&mut rng);
    let secp256k1_secret_key = SecretKey::random_secp256k1(&mut rng);

    let ed25519_public_key = PublicKey::from(&ed25519_secret_key);
    let secp256k1_public_key = PublicKey::from(&secp256k1_secret_key);

    let other_ed25519_public_key = PublicKey::random_ed25519(&mut rng);
    let other_secp256k1_public_key = PublicKey::random_secp256k1(&mut rng);

    let message = b"message";
    let ed25519_signature = sign(message, &ed25519_secret_key, &ed25519_public_key);
    let secp256k1_signature = sign(message, &secp256k1_secret_key, &secp256k1_public_key);

    assert!(verify(message, &ed25519_signature, &ed25519_public_key).is_ok());
    assert!(verify(message, &secp256k1_signature, &secp256k1_public_key).is_ok());

    assert!(verify(message, &ed25519_signature, &other_ed25519_public_key).is_err());
    assert!(verify(message, &secp256k1_signature, &other_secp256k1_public_key).is_err());

    assert!(verify(message, &ed25519_signature, &secp256k1_public_key).is_err());
    assert!(verify(message, &secp256k1_signature, &ed25519_public_key).is_err());

    assert!(verify(&message[1..], &ed25519_signature, &ed25519_public_key).is_err());
    assert!(verify(&message[1..], &secp256k1_signature, &secp256k1_public_key).is_err());
}

#[test]
fn should_construct_secp256k1_from_uncompressed_bytes() {
    let mut rng = TestRng::new();

    let mut secret_key_bytes = [0u8; SecretKey::SECP256K1_LENGTH];
    rng.fill_bytes(&mut secret_key_bytes[..]);

    // Construct a secp256k1 secret key and use that to construct a public key.
    let secp256k1_secret_key = k256::SecretKey::from_slice(&secret_key_bytes).unwrap();
    let secp256k1_public_key = secp256k1_secret_key.public_key();

    // Construct a CL secret key and public key from that (which will be a compressed key).
    let secret_key = SecretKey::secp256k1_from_bytes(secret_key_bytes).unwrap();
    let public_key = PublicKey::from(&secret_key);
    assert_eq!(
        Into::<Vec<u8>>::into(public_key.clone()).len(),
        PublicKey::SECP256K1_LENGTH
    );
    assert_ne!(
        secp256k1_public_key
            .to_encoded_point(false)
            .as_bytes()
            .len(),
        PublicKey::SECP256K1_LENGTH
    );

    // Construct a CL public key from uncompressed public key bytes and ensure it's compressed.
    let from_uncompressed_bytes =
        PublicKey::secp256k1_from_bytes(secp256k1_public_key.to_encoded_point(false).as_bytes())
            .unwrap();
    assert_eq!(public_key, from_uncompressed_bytes);

    // Construct a CL public key from the uncompressed one's hex representation and ensure it's
    // compressed.
    let uncompressed_hex = {
        let tag_bytes = vec![0x02u8];
        base16::encode_lower(&tag_bytes)
            + &base16::encode_lower(&secp256k1_public_key.to_encoded_point(false).as_bytes())
    };

    format!(
        "02{}",
        base16::encode_lower(secp256k1_public_key.to_encoded_point(false).as_bytes())
            .to_lowercase()
    );
    let from_uncompressed_hex = PublicKey::from_hex(uncompressed_hex).unwrap();
    assert_eq!(public_key, from_uncompressed_hex);
}

#[test]
fn generate_ed25519_should_generate_an_ed25519_key() {
    let secret_key = SecretKey::generate_ed25519().unwrap();
    assert!(matches!(secret_key, SecretKey::Ed25519(_)))
}

#[test]
fn generate_secp256k1_should_generate_an_secp256k1_key() {
    let secret_key = SecretKey::generate_secp256k1().unwrap();
    assert!(matches!(secret_key, SecretKey::Secp256k1(_)))
}
