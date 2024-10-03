use sha2::{Digest, Sha256};

/// The number of bytes in a hash.
/// NOTE: It does not make sense to use different lengths.
const DIGEST_LENGTH: usize = 32;

#[doc(hidden)]
pub fn blake3<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    let mut result = [0; DIGEST_LENGTH];
    let mut hasher = blake3::Hasher::new();

    hasher.update(data.as_ref());
    let hash = hasher.finalize();
    let hash_bytes: &[u8; DIGEST_LENGTH] = hash.as_bytes();
    result.copy_from_slice(hash_bytes);
    result
}

#[doc(hidden)]
pub fn sha256<T: AsRef<[u8]>>(data: T) -> [u8; DIGEST_LENGTH] {
    Sha256::digest(data).into()
}
