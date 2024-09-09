//! Implementation of the alt_bn128 curve operations.
use bn::{AffineG1, FieldError, Fq, Fr, Group, G1};
use casper_types::U256;
use thiserror::Error;

/// Errors that can occur when working with alt_bn128 curve.
#[derive(Debug, Error, PartialEq, Eq, PartialOrd, Ord)]
pub enum Error {
    /// Invalid length.
    #[error("Invalid length")]
    InvalidLength = 1,
    /// Invalid point x coordinate.
    #[error("Invalid point x coordinate")]
    InvalidXCoordinate = 2,
    /// Invalid point y coordinate.
    #[error("Invalid point y coordinate")]
    InvalidYCoordinate = 3,
    /// Invalid point.
    #[error("Invalid point")]
    InvalidPoint = 4,
    /// Invalid A.
    #[error("Invalid A")]
    InvalidA = 5,
    /// Invalid B.
    #[error("Invalid B")]
    InvalidB = 6,
    /// Invalid Ax.
    #[error("Invalid Ax")]
    InvalidAx = 7,
    /// Invalid Ay.
    #[error("Invalid Ay")]
    InvalidAy = 8,
    /// Invalid Bay.
    #[error("Invalid Bay")]
    InvalidBay = 9,
    /// Invalid Bax.
    #[error("Invalid Bax")]
    InvalidBax = 10,
    /// Invalid Bby.
    #[error("Invalid Bby")]
    InvalidBby = 11,
    /// Invalid Bbx.
    #[error("Invalid Bbx")]
    InvalidBbx = 12,
}

fn point_from_coords(x: U256, y: U256) -> Result<G1, Error> {
    let px = Fq::from_slice(&x.to_be_bytes()).map_err(|_| Error::InvalidXCoordinate)?;
    let py = Fq::from_slice(&y.to_be_bytes()).map_err(|_| Error::InvalidYCoordinate)?;

    Ok(if px == Fq::zero() && py == Fq::zero() {
        G1::zero()
    } else {
        AffineG1::new(px, py)
            .map_err(|_| Error::InvalidPoint)?
            .into()
    })
}

fn fq_to_u256(fq: Fq) -> U256 {
    let mut buf = [0u8; 32];
    fq.to_big_endian(&mut buf).unwrap();
    U256::from_big_endian(&buf)
}

fn fq_from_u256(u256: U256) -> Result<Fq, FieldError> {
    let mut buf = [0u8; 32];
    u256.to_big_endian(&mut buf);
    Fq::from_slice(&buf)
}

/// Adds two points on the alt_bn128 curve.
pub fn alt_bn128_add(x1: U256, y1: U256, x2: U256, y2: U256) -> Result<(U256, U256), Error> {
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

/// Multiplies a point on the alt_bn128 curve by a scalar.
pub fn alt_bn128_mul(x: U256, y: U256, scalar: U256) -> Result<(U256, U256), Error> {
    let p = point_from_coords(x, y)?;

    let mut x = U256::zero();
    let mut y = U256::zero();
    let fr = Fr::from_slice(&scalar.to_be_bytes()).map_err(|_| Error::InvalidPoint)?;

    if let Some(product) = AffineG1::from_jacobian(p * fr) {
        x = fq_to_u256(product.x());
        y = fq_to_u256(product.y());
    }

    Ok((x, y))
}

/// Pairing check for a list of points.
pub fn alt_bn128_pairing(values: Vec<(U256, U256, U256, U256, U256, U256)>) -> Result<bool, Error> {
    let mut pairs = Vec::with_capacity(values.len());
    for (ax, ay, bay, bax, bby, bbx) in values {
        let ax = fq_from_u256(ax).map_err(|_| Error::InvalidAx)?;
        let ay = fq_from_u256(ay).map_err(|_| Error::InvalidAy)?;
        let bay = fq_from_u256(bay).map_err(|_| Error::InvalidBay)?;
        let bax = fq_from_u256(bax).map_err(|_| Error::InvalidBax)?;
        let bby = fq_from_u256(bby).map_err(|_| Error::InvalidBby)?;
        let bbx = fq_from_u256(bbx).map_err(|_| Error::InvalidBbx)?;

        let g1_a = {
            if ax.is_zero() && ay.is_zero() {
                bn::G1::zero()
            } else {
                bn::AffineG1::new(ax, ay)
                    .map_err(|_| Error::InvalidA)?
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
                    .map_err(|_| Error::InvalidB)?
                    .into()
            }
        };

        pairs.push((g1_a, g1_b));
    }

    Ok(bn::pairing_batch(pairs.as_slice()) == bn::Gt::one())
}

#[cfg(test)]
mod tests {
    use casper_types::U256;

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
            Err(Error::InvalidPoint),
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
        assert_eq!(alt_bn128_mul(x, y, scalar), Err(Error::InvalidPoint));
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
        let bax_1 = U256::from_str_radix(
            "209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf7",
            16,
        )
        .unwrap();
        let bay_1 = U256::from_str_radix(
            "04bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a41678",
            16,
        )
        .unwrap();
        let bbx_1 = U256::from_str_radix(
            "2bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d",
            16,
        )
        .unwrap();
        let bby_1 = U256::from_str_radix(
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
        let bax_2 = U256::from_str_radix(
            "198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c2",
            16,
        )
        .unwrap();
        let bay_2 = U256::from_str_radix(
            "1800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed",
            16,
        )
        .unwrap();
        let bbx_2 = U256::from_str_radix(
            "090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b",
            16,
        )
        .unwrap();
        let bby_2 = U256::from_str_radix(
            "12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            16,
        )
        .unwrap();

        assert_eq!(
            alt_bn128_pairing(vec![
                (ax_1, ay_1, bax_1, bay_1, bbx_1, bby_1),
                (ax_2, ay_2, bax_2, bay_2, bbx_2, bby_2)
            ]),
            Ok(true)
        );
    }
}
