#[cfg(test)]
mod tests {
    use crate::{PublicKey, SecretKey};

    // TODO[RC]: Deduplicate
    fn random_bytes(len: usize) -> Vec<u8> {
        let mut buf = vec![0; len];
        getrandom::getrandom(&mut buf).expect("should get random");
        buf
    }

    #[test]
    fn era_report_capnp() {
        let bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key =
            SecretKey::secp256k1_from_bytes(bytes.as_slice()).expect("should create secret key");
        let equivocator_1: PublicKey = (&secret_key).into();

        let bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key =
            SecretKey::secp256k1_from_bytes(bytes.as_slice()).expect("should create secret key");
        let equivocator_2: PublicKey = (&secret_key).into();

        let bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key =
            SecretKey::secp256k1_from_bytes(bytes.as_slice()).expect("should create secret key");
        let inactive_validator_1: PublicKey = (&secret_key).into();

        let bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key =
            SecretKey::secp256k1_from_bytes(bytes.as_slice()).expect("should create secret key");
        let inactive_validator_2: PublicKey = (&secret_key).into();

        let bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key =
            SecretKey::secp256k1_from_bytes(bytes.as_slice()).expect("should create secret key");
        let got_reward_1: PublicKey = (&secret_key).into();

        let bytes = random_bytes(SecretKey::SECP256K1_LENGTH);
        let secret_key =
            SecretKey::secp256k1_from_bytes(bytes.as_slice()).expect("should create secret key");
        let got_reward_2: PublicKey = (&secret_key).into();

        //        let era_report = EraReport {};
    }
}
