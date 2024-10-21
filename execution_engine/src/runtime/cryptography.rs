//! Cryptography module containing hashing functions used internally
//! by the execution engine

use blake2::{
    digest::{Update, VariableOutput},
    Blake2bVar,
};
use sha2::{Digest, Sha256};

/// The number of bytes in a hash.
/// All hash functions in this module have a digest length of 32.
pub const DIGEST_LENGTH: usize = 32;

/// The 32-byte digest blake2b hash function
pub fn blake2b<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    let mut result = [0; DIGEST_LENGTH];
    // NOTE: Assumed safe as `BLAKE2B_DIGEST_LENGTH` is a valid value for a hasher
    let mut hasher = Blake2bVar::new(DIGEST_LENGTH).expect("should create hasher");

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
