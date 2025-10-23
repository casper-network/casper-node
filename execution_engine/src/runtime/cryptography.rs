//! Cryptography module containing hashing functions used internally
//! by the execution engine

use blake2::{
    digest::{Update, VariableOutput},
    Blake2bVar,
};
use keccak_asm::Digest as KeccakDigest;
use sha2::Sha256;

/// The number of bytes in a hash.
/// All hash functions in this module have a digest length of 32.
pub const DIGEST_LENGTH: usize = 32;

/// The 32-byte digest blake2b hash function
pub fn blake2b<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    let mut result = [0; DIGEST_LENGTH];
    // NOTE: Assumed safe as `DIGEST_LENGTH` is a valid value for a hasher
    // There is a test that ensures this
    let mut hasher =
        Blake2bVar::new(DIGEST_LENGTH).expect("Expected input to be a valid blake2 output length");

    hasher.update(data.as_ref());

    // NOTE: This should never fail, because result is exactly DIGEST_LENGTH long
    hasher.finalize_variable(&mut result).ok();

    result
}

/// The 32-byte digest blake3 hash function
pub fn blake3<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    let mut result = [0; DIGEST_LENGTH];
    let mut hasher = blake3::Hasher::new();

    hasher.update(data.as_ref());
    let hash = hasher.finalize();
    let hash_bytes: &[u8; DIGEST_LENGTH] = hash.as_bytes();
    result.copy_from_slice(hash_bytes);
    result
}

/// The 32-byte digest sha256 hash function
pub fn sha256<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    Sha256::digest(data).into()
}

/// The 32-byte digest keccak256 hash function
pub fn keccak256<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    use keccak_asm::Keccak256;

    let mut h = Keccak256::new();
    KeccakDigest::update(&mut h, &data);
    let mut out = [0u8; 32];
    let result = KeccakDigest::finalize(h);
    out.copy_from_slice(&result);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_blake2_uses_correct_output_size() {
        //This test is left here to make sure that the internal DIGEST_LENGTH
        // value doesn't change to a value that would make the blake2b function panic
        // If DIGEST_LENGTH would ever change it needs to change to a valid blake2 output value
        let output = super::blake2b(vec![1, 2, 5, 8]);
        assert_eq!(
            output,
            [
                131, 213, 124, 104, 118, 4, 132, 149, 47, 222, 254, 66, 213, 209, 147, 128, 223,
                25, 40, 239, 227, 222, 103, 98, 110, 93, 130, 169, 3, 110, 32, 213
            ]
        );
    }
}
