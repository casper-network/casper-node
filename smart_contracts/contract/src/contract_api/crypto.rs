//! Functions with cryptographic utils.

use casper_types::{api_error, HashAlgoType, BLAKE2B_DIGEST_LENGTH};

use crate::{ext_ffi, unwrap_or_revert::UnwrapOrRevert};

/// Computes digest hash, using provided algorithm type.
pub fn generic_hash<T: AsRef<[u8]>>(input: T, algo: HashAlgoType) -> [u8; 32] {
    let mut ret = [0; 32];

    let algo_ptr = &algo as *const _ as *const u8;

    let result = unsafe {
        ext_ffi::casper_generic_hash(
            input.as_ref().as_ptr(),
            input.as_ref().len(),
            algo_ptr,
            1, // HashAlgoType is just one byte (u8).
            ret.as_mut_ptr(),
            BLAKE2B_DIGEST_LENGTH,
        )
    };
    api_error::result_from(result).unwrap_or_revert();
    ret
}
