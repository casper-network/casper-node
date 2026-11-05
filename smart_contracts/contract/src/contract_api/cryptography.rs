//! Functions with cryptographic utils.

use casper_types::{
    api_error,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    ApiError, HashAlgorithm, PublicKey, Signature, BLAKE2B_DIGEST_LENGTH,
};

use crate::{ext_ffi, unwrap_or_revert::UnwrapOrRevert};

/// An alt_bn128 field coordinate encoded as a 32-byte little-endian integer.
pub type AltBn128Coordinate = [u8; 32];
/// An alt_bn128 scalar encoded as a 32-byte little-endian integer.
/// Multiplication reduces this integer modulo the group order.
pub type AltBn128Scalar = [u8; 32];
/// An alt_bn128 G1 point containing little-endian `[x, y]` coordinates.
/// Two zero coordinates represent the point at infinity.
pub type AltBn128Point = [AltBn128Coordinate; 2];
/// An alt_bn128 pairing entry containing `[ax, ay, bax, bay, bbx, bby]`.
/// All six coordinates are 32-byte little-endian integers. The G1 point is
/// `(ax, ay)` and the G2 point is `(Fq2(bax, bay), Fq2(bbx, bby))`, with each
/// Fq2 element ordered as `(real, imaginary)`. All-zero points represent infinity.
pub type AltBn128Pair = [AltBn128Coordinate; 6];

/// Adds two alt_bn128 G1 points, returning little-endian `[x, y]` coordinates.
/// Returns [`ApiError::InvalidArgument`] for an invalid coordinate or point.
pub fn alt_bn128_add(
    x1: &AltBn128Coordinate,
    y1: &AltBn128Coordinate,
    x2: &AltBn128Coordinate,
    y2: &AltBn128Coordinate,
) -> Result<AltBn128Point, ApiError> {
    let input = [*x1, *y1, *x2, *y2];
    let mut output = [[0; 32]; 2];
    let result = unsafe {
        ext_ffi::casper_alt_bn128_add(
            input.as_ptr().cast(),
            core::mem::size_of_val(&input),
            output.as_mut_ptr().cast(),
        )
    };
    api_error::result_from(result)?;
    Ok(output)
}

/// Multiplies an alt_bn128 G1 point by a little-endian scalar, returning
/// little-endian `[x, y]` coordinates. The scalar is reduced modulo the group order.
/// Returns [`ApiError::InvalidArgument`] for an invalid coordinate or point.
pub fn alt_bn128_mul(
    x: &AltBn128Coordinate,
    y: &AltBn128Coordinate,
    scalar: &AltBn128Scalar,
) -> Result<AltBn128Point, ApiError> {
    let input = [*x, *y, *scalar];
    let mut output = [[0; 32]; 2];
    let result = unsafe {
        ext_ffi::casper_alt_bn128_mul(
            input.as_ptr().cast(),
            core::mem::size_of_val(&input),
            output.as_mut_ptr().cast(),
        )
    };
    api_error::result_from(result)?;
    Ok(output)
}

/// Checks whether the product of alt_bn128 pairings is the identity.
/// An empty list returns `true`. Returns [`ApiError::InvalidArgument`] for an
/// invalid coordinate or point, including a G2 point outside the prime-order subgroup.
pub fn alt_bn128_pairing(pairs: &[AltBn128Pair]) -> Result<bool, ApiError> {
    let mut output = 0;
    let result = unsafe {
        ext_ffi::casper_alt_bn128_pairing(
            pairs.as_ptr().cast(),
            core::mem::size_of_val(pairs),
            &mut output,
        )
    };
    api_error::result_from(result)?;
    Ok(output != 0)
}

/// Computes digest hash, using provided algorithm type.
pub fn generic_hash<T: AsRef<[u8]>>(input: T, algo: HashAlgorithm) -> [u8; 32] {
    let mut ret = [0; 32];

    let result = unsafe {
        ext_ffi::casper_generic_hash(
            input.as_ref().as_ptr(),
            input.as_ref().len(),
            algo as u8,
            ret.as_mut_ptr(),
            BLAKE2B_DIGEST_LENGTH,
        )
    };
    api_error::result_from(result).unwrap_or_revert();
    ret
}

/// Attempts to recover a Secp256k1 [`PublicKey`] from a message and a signature over it.
pub fn recover_secp256k1<T: AsRef<[u8]>>(
    data: T,
    signature: &Signature,
    recovery_id: u8,
) -> Result<PublicKey, ApiError> {
    let mut buffer = [0; U8_SERIALIZED_LENGTH + PublicKey::SECP256K1_LENGTH];
    let signature_bytes = signature.to_bytes().unwrap_or_revert();

    let result = unsafe {
        ext_ffi::casper_recover_secp256k1(
            data.as_ref().as_ptr(),
            data.as_ref().len(),
            signature_bytes.as_ptr(),
            signature_bytes.len(),
            buffer.as_mut_ptr(),
            recovery_id,
        )
    };

    PublicKey::from_bytes(&buffer)
        .map(|(key, _)| key)
        .map_err(|_| ApiError::from(result as u32))
}

/// Verifies the signature of the given message against the given public key.
pub fn verify_signature<T: AsRef<[u8]>>(
    data: T,
    signature: &Signature,
    public_key: &PublicKey,
) -> Result<(), ApiError> {
    let signature_bytes = signature.to_bytes().unwrap_or_revert();
    let public_key_bytes = public_key.to_bytes().unwrap_or_revert();

    let result = unsafe {
        ext_ffi::casper_verify_signature(
            data.as_ref().as_ptr(),
            data.as_ref().len(),
            signature_bytes.as_ptr(),
            signature_bytes.len(),
            public_key_bytes.as_ptr(),
            public_key_bytes.len(),
        )
    };

    api_error::result_from(result)
}
