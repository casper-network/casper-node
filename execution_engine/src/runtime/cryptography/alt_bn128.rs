//! Fixed-width, little-endian alt_bn128 operations for the VM1 host interface.

use bn::{AffineG1, AffineG2, Fq, Fq2, Fr, Group, G1, G2};
use casper_types::ApiError;

const COORDINATE_SIZE: usize = 32;
const POINT_SIZE: usize = 2 * COORDINATE_SIZE;
const PAIR_SIZE: usize = 6 * COORDINATE_SIZE;

#[cfg(test)]
mod benchmarks;

fn big_endian(bytes: &[u8]) -> Result<[u8; COORDINATE_SIZE], ApiError> {
    let mut bytes: [u8; COORDINATE_SIZE] =
        bytes.try_into().map_err(|_| ApiError::InvalidArgument)?;
    bytes.reverse();
    Ok(bytes)
}

fn field(bytes: &[u8]) -> Result<Fq, ApiError> {
    Fq::from_slice(&big_endian(bytes)?).map_err(|_| ApiError::InvalidArgument)
}

fn point(x: Fq, y: Fq) -> Result<G1, ApiError> {
    if x.is_zero() && y.is_zero() {
        Ok(G1::zero())
    } else {
        AffineG1::new(x, y)
            .map(Into::into)
            .map_err(|_| ApiError::InvalidArgument)
    }
}

fn read_point(bytes: &[u8]) -> Result<G1, ApiError> {
    point(
        field(&bytes[..COORDINATE_SIZE])?,
        field(&bytes[COORDINATE_SIZE..])?,
    )
}

fn write_point(point: G1) -> Result<[u8; POINT_SIZE], ApiError> {
    let mut output = [0; POINT_SIZE];
    if let Some(point) = AffineG1::from_jacobian(point) {
        for (coordinate, dest) in [point.x(), point.y()]
            .into_iter()
            .zip(output.as_chunks_mut::<COORDINATE_SIZE>().0)
        {
            coordinate
                .to_big_endian(dest)
                .map_err(|_| ApiError::InvalidArgument)?;
            dest.reverse();
        }
    }
    Ok(output)
}

pub(in crate::runtime) fn add(input: &[u8]) -> Result<[u8; POINT_SIZE], ApiError> {
    if input.len() != 2 * POINT_SIZE {
        return Err(ApiError::InvalidArgument);
    }
    write_point(read_point(&input[..POINT_SIZE])? + read_point(&input[POINT_SIZE..])?)
}

pub(in crate::runtime) fn mul(input: &[u8]) -> Result<[u8; POINT_SIZE], ApiError> {
    if input.len() != POINT_SIZE + COORDINATE_SIZE {
        return Err(ApiError::InvalidArgument);
    }
    let point = read_point(&input[..POINT_SIZE])?;
    // As in VM2, Fr::from_slice reduces the full 256-bit scalar modulo the group order.
    let scalar = Fr::from_slice(&big_endian(&input[POINT_SIZE..])?)
        .map_err(|_| ApiError::InvalidArgument)?;
    write_point(point * scalar)
}

pub(in crate::runtime) fn pairing(input: &[u8]) -> Result<bool, ApiError> {
    let (entries, remainder) = input.as_chunks::<PAIR_SIZE>();
    if !remainder.is_empty() {
        return Err(ApiError::InvalidArgument);
    }
    let mut pairs = Vec::with_capacity(entries.len());
    for pair in entries {
        let a = read_point(&pair[..POINT_SIZE])?;
        // The Fq2 components are (bax, bay) for x and (bbx, bby) for y.
        let b = &pair[POINT_SIZE..];
        let x = Fq2::new(field(&b[..32])?, field(&b[32..64])?);
        let y = Fq2::new(field(&b[64..96])?, field(&b[96..])?);
        let b = if x.is_zero() && y.is_zero() {
            G2::zero()
        } else {
            AffineG2::new(x, y)
                .map_err(|_| ApiError::InvalidArgument)?
                .into()
        };
        pairs.push((a, b));
    }
    Ok(bn::pairing_batch(&pairs) == bn::Gt::one())
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::U256;

    #[test]
    fn rejects_malformed_lengths() {
        for size in [0, 1, 95, 97, 127, 129, 191, 193] {
            assert_eq!(add(&vec![0; size]), Err(ApiError::InvalidArgument));
            assert_eq!(mul(&vec![0; size]), Err(ApiError::InvalidArgument));
            if size != 0 {
                assert_eq!(pairing(&vec![0; size]), Err(ApiError::InvalidArgument));
            }
        }
    }

    #[test]
    fn rejects_noncanonical_coordinates() {
        let mut modulus = [0; 32];
        Fq::modulus().to_big_endian(&mut modulus).unwrap();
        modulus.reverse();
        for coordinate in [modulus, [0xff; 32]] {
            for index in 0..4 {
                let mut input = [0; 128];
                input[32 * index..32 * (index + 1)].copy_from_slice(&coordinate);
                assert_eq!(add(&input), Err(ApiError::InvalidArgument));
            }
            for index in 0..2 {
                let mut input = [0; 96];
                input[32 * index..32 * (index + 1)].copy_from_slice(&coordinate);
                assert_eq!(mul(&input), Err(ApiError::InvalidArgument));
            }
            for index in 0..6 {
                let mut input = [0; 192];
                input[32 * index..32 * (index + 1)].copy_from_slice(&coordinate);
                assert_eq!(pairing(&input), Err(ApiError::InvalidArgument));
            }
        }
    }

    #[test]
    fn scalar_reduction_and_identity() {
        let one = U256::one();
        let two = U256::from(2);
        let zero = U256::zero();
        let order = U256::from_str_radix(
            "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001",
            16,
        )
        .unwrap();
        assert_eq!(alt_bn128_add(one, two, zero, zero), Ok((one, two)));
        assert_eq!(alt_bn128_mul(one, two, zero), Ok((zero, zero)));
        assert_eq!(alt_bn128_mul(one, two, order), Ok((zero, zero)));
        assert_eq!(alt_bn128_mul(one, two, order + one), Ok((one, two)));
        let reduced = U256::MAX % order;
        assert_eq!(
            alt_bn128_mul(one, two, U256::MAX),
            alt_bn128_mul(one, two, reduced)
        );
    }

    #[test]
    fn pairing_identity_false_and_invalid_g2() {
        assert_eq!(pairing(&[]), Ok(true));
        assert_eq!(pairing(&[0; 192]), Ok(true));
        let g2 = AffineG2::from_jacobian(G2::one()).unwrap();
        let mut input = [0; 192];
        input[0] = 1;
        input[32] = 2;
        for (coordinate, dest) in [
            g2.x().real(),
            g2.x().imaginary(),
            g2.y().real(),
            g2.y().imaginary(),
        ]
        .into_iter()
        .zip(input[64..].as_chunks_mut::<32>().0)
        {
            coordinate.to_big_endian(dest).unwrap();
            dest.reverse();
        }
        assert_eq!(pairing(&input), Ok(false));
        input[64..].fill(0);
        input[64] = 1;
        input[128] = 1;
        assert_eq!(pairing(&input), Err(ApiError::InvalidArgument));
    }

    #[test]
    fn rejects_g2_points_outside_the_prime_order_subgroup() {
        // Find a point on the twist, without projecting it into the prime-order subgroup.
        let (x, y) = (0..256)
            .find_map(|value| {
                let x = Fq2::new(Fq::from_str(&value.to_string()).unwrap(), Fq::one());
                let y = (x * x * x + G2::b()).sqrt()?;
                AffineG2::new(x, y).is_err().then_some((x, y))
            })
            .expect("should find a twist point outside the subgroup");
        assert_eq!(y * y, x * x * x + G2::b());
        let mut input = [0; 192];
        for (coordinate, dest) in [x.real(), x.imaginary(), y.real(), y.imaginary()]
            .into_iter()
            .zip(input[64..].as_chunks_mut::<32>().0)
        {
            coordinate.to_big_endian(dest).unwrap();
            dest.reverse();
        }
        // Even a zero G1 point must not bypass validation of its G2 partner.
        assert_eq!(pairing(&input), Err(ApiError::InvalidArgument));
    }

    fn coordinates(values: &[U256]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| {
                let mut bytes = [0; 32];
                value.to_little_endian(&mut bytes);
                bytes
            })
            .collect()
    }

    fn result_point(bytes: [u8; 64]) -> (U256, U256) {
        (
            U256::from_little_endian(&bytes[..32]),
            U256::from_little_endian(&bytes[32..]),
        )
    }

    fn alt_bn128_add(x1: U256, y1: U256, x2: U256, y2: U256) -> Result<(U256, U256), ApiError> {
        add(&coordinates(&[x1, y1, x2, y2])).map(result_point)
    }

    fn alt_bn128_mul(x: U256, y: U256, scalar: U256) -> Result<(U256, U256), ApiError> {
        mul(&coordinates(&[x, y, scalar])).map(result_point)
    }

    fn alt_bn128_pairing(
        values: Vec<(U256, U256, U256, U256, U256, U256)>,
    ) -> Result<bool, ApiError> {
        let values: Vec<_> = values
            .into_iter()
            .flat_map(|(ax, ay, bax, bay, bbx, bby)| [ax, ay, bax, bay, bbx, bby])
            .collect();
        pairing(&coordinates(&values))
    }

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
            Err(ApiError::InvalidArgument),
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
        assert_eq!(alt_bn128_mul(x, y, scalar), Err(ApiError::InvalidArgument));
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
