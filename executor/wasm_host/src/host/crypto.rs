use blake2::Blake2bVar;
use bn::{AffineG1, Fq, Fr, Group, G1};
use borsh::{BorshDeserialize, BorshSerialize};
use bytes::Bytes;
use casper_executor_wasm_common::error::{
    HOST_ERROR_INVALID_DATA, HOST_ERROR_INVALID_INPUT, HOST_ERROR_PAYLOAD_TOO_LONG,
    HOST_ERROR_SUCCESS,
};
use casper_executor_wasm_interface::{executor::ExecuteError, FatalHostError, VMError, VMResult};
use casper_types::{
    bytesrepr::{self, Bytes as BytesreprBytes, FromBytes, ToBytes},
    HashAlgorithm, Signature, U256,
};
use keccak_asm::Digest as KeccakDigest;
use num_traits::FromPrimitive;
use sha2::{
    digest::{Update, VariableOutput},
    Sha256,
};
use thiserror::Error as ThisError;
use tracing::debug;

#[derive(Clone, BorshSerialize, BorshDeserialize, Debug)]
pub struct Pair {
    /// G1 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    ax: [u8; 32],
    /// G1 point y-coordinate. 32 bytes little-endian encoded unsigned integer
    ay: [u8; 32],
    /// G2 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    bax: [u8; 32],
    /// G2 point y-coordinate. 32 bytes little-endian encoded unsigned integer
    bay: [u8; 32],
    /// G1 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    bbx: [u8; 32],
    /// G1 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    bby: [u8; 32],
}

impl FromBytes for Pair {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (ax, remainder) = <[u8; 32]>::from_bytes(bytes)?;
        let (ay, remainder) = <[u8; 32]>::from_bytes(remainder)?;
        let (bax, remainder) = <[u8; 32]>::from_bytes(remainder)?;
        let (bay, remainder) = <[u8; 32]>::from_bytes(remainder)?;
        let (bbx, remainder) = <[u8; 32]>::from_bytes(remainder)?;
        let (bby, remainder) = <[u8; 32]>::from_bytes(remainder)?;
        Ok((
            Pair {
                ax,
                ay,
                bax,
                bay,
                bbx,
                bby,
            },
            remainder,
        ))
    }
}

impl Pair {
    pub fn to_u256_tuples(&self) -> (U256, U256, U256, U256, U256, U256) {
        (
            U256::from_little_endian(&self.ax),
            U256::from_little_endian(&self.ay),
            U256::from_little_endian(&self.bax),
            U256::from_little_endian(&self.bay),
            U256::from_little_endian(&self.bbx),
            U256::from_little_endian(&self.bby),
        )
    }
}

/// Errors that can occur when working with alt_bn128 curve.
/// We start numbering altbn128 specific errors from 100,
/// since the lower ones we reserve for "generic processing"
/// errors
#[derive(Debug, ThisError, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AltBN128Error {
    /// Invalid point x coordinate.
    #[error("Invalid point x coordinate")]
    InvalidXCoordinate = 100,
    /// Invalid point y coordinate.
    #[error("Invalid point y coordinate")]
    InvalidYCoordinate = 101,
    /// Invalid point.
    #[error("Invalid point")]
    InvalidPoint = 102,
    /// Invalid A.
    #[error("Invalid A")]
    InvalidA = 103,
    /// Invalid B.
    #[error("Invalid B")]
    InvalidB = 104,
    /// Invalid Ax.
    #[error("Invalid Ax")]
    InvalidAx = 105,
    /// Invalid Ay.
    #[error("Invalid Ay")]
    InvalidAy = 106,
    /// Invalid Bay.
    #[error("Invalid Bay")]
    InvalidBay = 107,
    /// Invalid Bax.
    #[error("Invalid Bax")]
    InvalidBax = 108,
    /// Invalid Bby.
    #[error("Invalid Bby")]
    InvalidBby = 109,
    /// Invalid Bbx.
    #[error("Invalid Bbx")]
    InvalidBbx = 110,
}

pub(crate) fn host_alt_bn128_add(input: Bytes) -> VMResult<(Option<Bytes>, u32)> {
    let (x1_bytes, y1_bytes, x2_bytes, y2_bytes) = match bytesrepr::deserialize_from_slice::<
        &Bytes,
        ([u8; 32], [u8; 32], [u8; 32], [u8; 32]),
    >(&input)
    {
        Ok(res) => res,
        Err(_) => {
            return Ok((None, HOST_ERROR_INVALID_INPUT));
        }
    };
    let x1 = U256::from_little_endian(&x1_bytes);
    let y1 = U256::from_little_endian(&y1_bytes);
    let x2 = U256::from_little_endian(&x2_bytes);
    let y2 = U256::from_little_endian(&y2_bytes);
    let res = alt_bn128_add(x1, y1, x2, y2)
        .map(|(x, y)| {
            let mut output = Vec::with_capacity(64);
            let mut x_buf = [0u8; 32];
            let mut y_buf = [0u8; 32];
            x.to_little_endian(&mut x_buf);
            y.to_little_endian(&mut y_buf);
            output.extend_from_slice(&x_buf);
            output.extend_from_slice(&y_buf);
            output
        })
        .map_err(|err| err as u32);
    match res {
        Ok(bytes) => Ok((Some(bytes.into()), HOST_ERROR_SUCCESS)),
        Err(err_code) => Ok((None, err_code)),
    }
}

pub(crate) fn host_alt_bn128_mul(input: Bytes) -> VMResult<(Option<Bytes>, u32)> {
    let (x_bytes, y_bytes, scalar_bytes) =
        match bytesrepr::deserialize_from_slice::<&Bytes, ([u8; 32], [u8; 32], [u8; 32])>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok((None, HOST_ERROR_INVALID_INPUT));
            }
        };
    let x = U256::from_little_endian(&x_bytes);
    let y = U256::from_little_endian(&y_bytes);
    let scalar = U256::from_little_endian(&scalar_bytes);
    let res = alt_bn128_mul(x, y, scalar)
        .map(|(x, y)| {
            let mut output = Vec::with_capacity(64);
            let mut x_buf = [0u8; 32];
            let mut y_buf = [0u8; 32];
            x.to_little_endian(&mut x_buf);
            y.to_little_endian(&mut y_buf);
            output.extend_from_slice(&x_buf);
            output.extend_from_slice(&y_buf);
            output
        })
        .map_err(|err| err as u32);
    match res {
        Ok(bytes) => Ok((Some(bytes.into()), HOST_ERROR_SUCCESS)),
        Err(err_code) => Ok((None, err_code)),
    }
}

pub(crate) fn host_alt_bn128_pairing(input: Bytes) -> VMResult<(Option<Bytes>, u32)> {
    let pairs = match bytesrepr::deserialize_from_slice::<&Bytes, Vec<Pair>>(&input) {
        Ok(res) => res,
        Err(_) => {
            return Ok((None, HOST_ERROR_INVALID_INPUT));
        }
    };
    let values = pairs.iter().map(Pair::to_u256_tuples).collect();
    let res = alt_bn128_pairing(values).map_err(|e| e as u32);
    match res {
        Ok(is_paired) => match is_paired.to_bytes() {
            Ok(bytes) => Ok((Some(bytes.into()), HOST_ERROR_SUCCESS)),
            Err(_) => Err(VMError::Fatal(FatalHostError::TypeConversion)),
        },
        Err(err_code) => Ok((None, err_code)),
    }
}

/// Computes digest hash, using provided algorithm type.
///
/// # Arguments
/// - `input` byte array consisting of:
///     - `hash_algorithm`: 4 bytes deserialized as `u32` and interpreted as [HashAlgorithm].
///     - `in_bytes`: an array of bytes serialized in a bytesrepr-compatible way. This will be
///       interpreted as the payload to sign.
pub(crate) fn host_generic_hash(input: Bytes) -> VMResult<(Option<Bytes>, u32)> {
    let (hash_algorithm, in_bytes) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u32, BytesreprBytes)>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok((None, HOST_ERROR_INVALID_INPUT));
            }
        };

    const DIGEST_LENGTH: usize = 32;

    let hash_algorithm = match HashAlgorithm::from_u32(hash_algorithm) {
        Some(alg) => alg,
        None => return Ok((None, HOST_ERROR_INVALID_INPUT)),
    };

    let hashed_bytes = match hash_algorithm {
        HashAlgorithm::Blake2b => {
            let mut result = [0; DIGEST_LENGTH];
            let mut hasher = Blake2bVar::new(DIGEST_LENGTH).map_err(|_| {
                ExecuteError::Fatal(FatalHostError::CorruptExecutionState(
                    "Error when creating instance of Blake2bVar hashing".to_owned(),
                ))
            })?;
            hasher.update(in_bytes.as_ref());
            hasher.finalize_variable(&mut result).ok();
            result
        }
        HashAlgorithm::Blake3 => {
            let mut result = [0; DIGEST_LENGTH];
            let mut hasher = blake3::Hasher::new();
            hasher.update(in_bytes.as_ref());
            let hash = hasher.finalize();
            let hash_bytes: &[u8; DIGEST_LENGTH] = hash.as_bytes();
            result.copy_from_slice(hash_bytes);
            result
        }
        HashAlgorithm::Sha256 => Sha256::digest(in_bytes).into(),
        HashAlgorithm::Keccak256 => {
            use keccak_asm::Keccak256;
            let mut result = [0u8; DIGEST_LENGTH];
            let mut hasher = Keccak256::new();
            KeccakDigest::update(&mut hasher, &in_bytes);
            let hash = KeccakDigest::finalize(hasher);
            result.copy_from_slice(&hash);
            result
        }
    };

    Ok((Some(hashed_bytes.to_vec().into()), HOST_ERROR_SUCCESS))
}

/// Recovers a Secp256k1 public key from a signed message
/// and a signature used in the process of signing.
///
/// # Arguments
/// - `input` is a byte array consisting of:
///     - `recovery_id`: 4 bytes deserialized as `u32`. valid values: 0_u32, 1_u32, 2_u32, 3_u32.
///       Interpretation for these `recovery_id` values is as follows:
///           - Low bit (0/1): was the y-coordinate of the affine point resulting from the
///             fixed-base multiplication 𝑘×𝑮 odd?
///            - Hi bit (3/4): did the affine x-coordinate of 𝑘×𝑮 overflow the order of the scalar
///              field,
///             requiring a reduction when computing r?
///     - `message`: an array of bytes serialized in a bytesrepr-compatible way. This will be
///       interpreted as the signed data.
///     - `signature`: an array of bytes serialized in a bytesrepr-compatible way. This will be
///       interpreted as the signature for the `message`.
///
/// # Output is one of:
///  - Err(VmError): if a internal vm error happened (not related to the imputed data)
///  - Ok(None, err_code): with err_code > 0. Err_code represents a problem with processing imputed
///    data.
///  - Ok(Some(pk_bytes), 0): `pk_bytes` is a serialized [PublicKey] recovered for the signature
pub(crate) fn host_recover_secp256k1(input: Bytes) -> VMResult<(Option<Bytes>, u32)> {
    let (recovery_id, message, signature_bytes) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u32, BytesreprBytes, BytesreprBytes)>(
            &input,
        ) {
            Ok(res) => res,
            Err(_) => {
                return Ok((None, HOST_ERROR_INVALID_INPUT));
            }
        };

    if recovery_id >= 4 {
        return Ok((None, HOST_ERROR_INVALID_INPUT));
    }

    let Ok((signature, _)) = Signature::from_bytes(&signature_bytes) else {
        return Ok((None, HOST_ERROR_INVALID_DATA));
    };

    let Ok(public_key) =
        casper_types::crypto::recover_secp256k1(message, &signature, recovery_id as u8)
    else {
        return Ok((None, HOST_ERROR_INVALID_INPUT));
    };

    let Ok(key_bytes) = public_key.to_bytes() else {
        return Ok((None, HOST_ERROR_PAYLOAD_TOO_LONG));
    };

    Ok((Some(key_bytes.into()), HOST_ERROR_SUCCESS))
}

fn alt_bn128_add(x1: U256, y1: U256, x2: U256, y2: U256) -> Result<(U256, U256), AltBN128Error> {
    let p1 = point_from_coords(x1, y1)?;
    let p2 = point_from_coords(x2, y2)?;
    let mut x = U256::zero();
    let mut y = U256::zero();

    if let Some(sum) = AffineG1::from_jacobian(p1 + p2) {
        x = fq_to_u256(sum.x());
        y = fq_to_u256(sum.y());
    }
    Ok((x, y))
}

fn alt_bn128_mul(x: U256, y: U256, scalar: U256) -> Result<(U256, U256), AltBN128Error> {
    let p = point_from_coords(x, y)?;

    let mut x = U256::zero();
    let mut y = U256::zero();
    let mut buf = [0u8; 32];
    scalar.to_big_endian(&mut buf);
    let fr = Fr::from_slice(&buf).map_err(|_| AltBN128Error::InvalidPoint)?;

    if let Some(product) = AffineG1::from_jacobian(p * fr) {
        x = fq_to_u256(product.x());
        y = fq_to_u256(product.y());
    }

    Ok((x, y))
}

/// Pairing check for a list of points.
fn alt_bn128_pairing(
    values: Vec<(U256, U256, U256, U256, U256, U256)>,
) -> Result<bool, AltBN128Error> {
    let mut pairs = Vec::with_capacity(values.len());
    for (ax, ay, bax, bay, bbx, bby) in values {
        let ax = fq_from_u256(ax).map_err(|_| AltBN128Error::InvalidAx)?;
        let ay = fq_from_u256(ay).map_err(|_| AltBN128Error::InvalidAy)?;
        let bax = fq_from_u256(bax).map_err(|_| AltBN128Error::InvalidBax)?;
        let bay = fq_from_u256(bay).map_err(|_| AltBN128Error::InvalidBay)?;
        let bbx = fq_from_u256(bbx).map_err(|_| AltBN128Error::InvalidBbx)?;
        let bby = fq_from_u256(bby).map_err(|_| AltBN128Error::InvalidBby)?;

        let g1_a = {
            if ax.is_zero() && ay.is_zero() {
                bn::G1::zero()
            } else {
                bn::AffineG1::new(ax, ay)
                    .map_err(|_| AltBN128Error::InvalidA)?
                    .into()
            }
        };
        let g1_b = {
            let ba = bn::Fq2::new(bax, bay);
            let bb = bn::Fq2::new(bbx, bby);

            if ba.is_zero() && bb.is_zero() {
                bn::G2::zero()
            } else {
                bn::AffineG2::new(ba, bb)
                    .map_err(|_| AltBN128Error::InvalidB)?
                    .into()
            }
        };
        pairs.push((g1_a, g1_b));
    }

    Ok(bn::pairing_batch(pairs.as_slice()) == bn::Gt::one())
}

pub(super) fn point_from_coords(x: U256, y: U256) -> Result<G1, AltBN128Error> {
    let mut buf = [0u8; 32];
    x.to_big_endian(&mut buf);
    let px = Fq::from_slice(&buf).map_err(|_| AltBN128Error::InvalidXCoordinate)?;
    let mut buf = [0u8; 32];
    y.to_big_endian(&mut buf);
    let py = Fq::from_slice(&buf).map_err(|_| AltBN128Error::InvalidYCoordinate)?;

    Ok(if px == Fq::zero() && py == Fq::zero() {
        G1::zero()
    } else {
        AffineG1::new(px, py)
            .map_err(|_| AltBN128Error::InvalidPoint)?
            .into()
    })
}

fn fq_to_u256(fq: Fq) -> U256 {
    let mut buf = [0u8; 32];
    fq.to_big_endian(&mut buf).unwrap();
    U256::from_big_endian(&buf)
}

enum FqDeserializationError {
    FailedToDeserialize,
}

fn fq_from_u256(value: U256) -> Result<Fq, FqDeserializationError> {
    let mut buf = [0u8; 32];
    value.to_big_endian(&mut buf);
    let arith_u256 = bn::arith::U256::from_slice(&buf).map_err(|_| {
        debug!("Failed to build bn::arith::U256 from casper_types:U256");
        FqDeserializationError::FailedToDeserialize
    })?;
    Fq::from_u256(arith_u256).map_err(|_| {
        debug!("Failed to build Fq");
        FqDeserializationError::FailedToDeserialize
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alt_bn128_add() {
        let x1 = U256::from_str_radix(
            "18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9",
            16,
        )
        .unwrap();

        let y1 = U256::from_str_radix(
            "063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266",
            16,
        )
        .unwrap();

        let x2 = U256::from_str_radix(
            "07c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed",
            16,
        )
        .unwrap();
        let y2 = U256::from_str_radix(
            "06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7",
            16,
        )
        .unwrap();

        let expected_x = U256::from_str_radix(
            "2243525c5efd4b9c3d3c45ac0ca3fe4dd85e830a4ce6b65fa1eeaee202839703",
            16,
        )
        .unwrap();
        let expected_y = U256::from_str_radix(
            "301d1d33be6da8e509df21cc35964723180eed7532537db9ae5e7d48f195c915",
            16,
        )
        .unwrap();

        let result = alt_bn128_add(x1, y1, x2, y2);
        assert_eq!(result, Ok((expected_x, expected_y)));
    }

    #[test]
    fn zero() {
        assert_eq!(
            alt_bn128_add(U256::zero(), U256::zero(), U256::zero(), U256::zero()),
            Ok((U256::zero(), U256::zero()))
        );
    }

    #[test]
    fn add_error() {
        let all_ones = U256::from_str_radix(
            "1111111111111111111111111111111111111111111111111111111111111111",
            16,
        )
        .unwrap();

        assert_eq!(
            alt_bn128_add(all_ones, all_ones, all_ones, all_ones),
            Err(AltBN128Error::InvalidPoint),
        );
    }

    #[test]
    fn test_alt_bn128_mul() {
        let x = U256::from_str_radix(
            "2bd3e6d0f3b142924f5ca7b49ce5b9d54c4703d7ae5648e61d02268b1a0a9fb7",
            16,
        )
        .unwrap();
        let y = U256::from_str_radix(
            "21611ce0a6af85915e2f1d70300909ce2e49dfad4a4619c8390cae66cefdb204",
            16,
        )
        .unwrap();
        let scalar = U256::from_str_radix(
            "00000000000000000000000000000000000000000000000011138ce750fa15c2",
            16,
        )
        .unwrap();

        let expected_x = U256::from_str_radix(
            "070a8d6a982153cae4be29d434e8faef8a47b274a053f5a4ee2a6c9c13c31e5c",
            16,
        )
        .unwrap();
        let expected_y = U256::from_str_radix(
            "031b8ce914eba3a9ffb989f9cdd5b0f01943074bf4f0f315690ec3cec6981afc",
            16,
        )
        .unwrap();

        assert_eq!(alt_bn128_mul(x, y, scalar), Ok((expected_x, expected_y)));
    }

    #[test]
    fn test_zero_multiplication() {
        // zero multiplication test

        let x = U256::from_str_radix(
            "0000000000000000000000000000000000000000000000000000000000000000",
            16,
        )
        .unwrap();
        let y = U256::from_str_radix(
            "0000000000000000000000000000000000000000000000000000000000000000",
            16,
        )
        .unwrap();
        let scalar = U256::from_str_radix(
            "0200000000000000000000000000000000000000000000000000000000000000",
            16,
        )
        .unwrap();

        let expected_x = U256::from_str_radix(
            "0000000000000000000000000000000000000000000000000000000000000000",
            16,
        )
        .unwrap();

        let expected_y = U256::from_str_radix(
            "0000000000000000000000000000000000000000000000000000000000000000",
            16,
        )
        .unwrap();

        assert_eq!(alt_bn128_mul(x, y, scalar), Ok((expected_x, expected_y)));
    }

    #[test]
    fn test_not_on_curve_multiplication() {
        // point not on curve fail

        let x = U256::from_str_radix(
            "1111111111111111111111111111111111111111111111111111111111111111",
            16,
        )
        .unwrap();
        let y = U256::from_str_radix(
            "1111111111111111111111111111111111111111111111111111111111111111",
            16,
        )
        .unwrap();
        let scalar = U256::from_str_radix(
            "0f00000000000000000000000000000000000000000000000000000000000000",
            16,
        )
        .unwrap();
        assert_eq!(
            alt_bn128_mul(x, y, scalar),
            Err(AltBN128Error::InvalidPoint)
        );
    }

    #[test]
    fn test_pairing() {
        let ax_1 = U256::from_str_radix(
            "1c76476f4def4bb94541d57ebba1193381ffa7aa76ada664dd31c16024c43f59",
            16,
        )
        .unwrap();
        let ay_1 = U256::from_str_radix(
            "3034dd2920f673e204fee2811c678745fc819b55d3e9d294e45c9b03a76aef41",
            16,
        )
        .unwrap();
        let bay_1 = U256::from_str_radix(
            "209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf7",
            16,
        )
        .unwrap();
        let bax_1 = U256::from_str_radix(
            "04bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a41678",
            16,
        )
        .unwrap();
        let bby_1 = U256::from_str_radix(
            "2bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d",
            16,
        )
        .unwrap();
        let bbx_1 = U256::from_str_radix(
            "120a2a4cf30c1bf9845f20c6fe39e07ea2cce61f0c9bb048165fe5e4de877550",
            16,
        )
        .unwrap();

        let ax_2 = U256::from_str_radix(
            "111e129f1cf1097710d41c4ac70fcdfa5ba2023c6ff1cbeac322de49d1b6df7c",
            16,
        )
        .unwrap();
        let ay_2 = U256::from_str_radix(
            "2032c61a830e3c17286de9462bf242fca2883585b93870a73853face6a6bf411",
            16,
        )
        .unwrap();
        let bay_2 = U256::from_str_radix(
            "198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c2",
            16,
        )
        .unwrap();
        let bax_2 = U256::from_str_radix(
            "1800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed",
            16,
        )
        .unwrap();
        let bby_2 = U256::from_str_radix(
            "090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b",
            16,
        )
        .unwrap();
        let bbx_2 = U256::from_str_radix(
            "12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            16,
        )
        .unwrap();

        let result = alt_bn128_pairing(vec![
            (ax_1, ay_1, bax_1, bay_1, bbx_1, bby_1),
            (ax_2, ay_2, bax_2, bay_2, bbx_2, bby_2),
        ]);
        assert!(result.expect("Pairing failed"));
    }
}
