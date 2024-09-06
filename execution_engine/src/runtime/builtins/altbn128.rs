//! Implementation of the alt_bn128 curve operations.
use bn::{AffineG1, Fq, Group, GroupError, G1};
use casper_types::U256;
use thiserror::Error;

/// Errors that can occur when working with alt_bn128 curve.
#[derive(Debug, Error, PartialEq, Eq, PartialOrd, Ord)]
pub enum Error {
    /// Invalid point x coordinate.
    #[error("Invalid point x coordinate")]
    InvalidXCoordinate = 1,
    /// Invalid point y coordinate.
    #[error("Invalid point y coordinate")]
    InvalidYCoordinate = 2,
    /// Invalid point.
    #[error("Invalid point")]
    InvalidPoint = 3,
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
}
