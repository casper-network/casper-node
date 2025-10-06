//! Host-optimized support for pairing cryptography with the Barreto-Naehrig curve
use core::array::TryFromSliceError;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    casper::casper_system,
    types::{CryptoFunctionOption, U256},
};

/// A point on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, BorshSerialize, BorshDeserialize)]
#[repr(C, packed)]
pub struct G1([u8; 32]);

impl G1 {
    pub const SIZE_IN_BYTES: u32 = core::mem::size_of::<G1>() as u32;
    /// Returns a point with all bytes set to zero.
    pub const fn zero() -> Self {
        G1([0; 32])
    }

    /// Returns the inner byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for G1 {
    fn from(value: [u8; 32]) -> Self {
        G1(value)
    }
}

impl From<U256> for G1 {
    fn from(value: U256) -> Self {
        let bytes = u256_to_le_bytes(value);
        G1(bytes)
    }
}

/// A scalar on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, BorshSerialize, BorshDeserialize)]
#[repr(C, packed)]
pub struct Fr([u8; 32]);

impl Fr {
    pub const SIZE_IN_BYTES: u32 = core::mem::size_of::<Fr>() as u32;

    /// Returns a point with all bytes set to zero.
    pub const fn zero() -> Self {
        Fr([0; 32])
    }

    /// Returns the inner byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<U256> for Fr {
    fn from(value: U256) -> Self {
        let bytes = u256_to_le_bytes(value);
        Self(bytes)
    }
}

impl From<[u8; 32]> for Fr {
    fn from(value: [u8; 32]) -> Self {
        Fr(value)
    }
}

/// A field element on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, BorshSerialize, BorshDeserialize)]
#[repr(C, packed)]
pub struct Fq([u8; 32]);

impl Fq {
    /// Returns a point with all bytes set to zero.
    pub const fn zero() -> Self {
        Fq([0; 32])
    }

    /// Returns the inner byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<U256> for Fq {
    fn from(value: U256) -> Self {
        let bytes = u256_to_le_bytes(value);
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for Fq {
    type Error = TryFromSliceError;

    fn try_from(value: &[u8]) -> core::result::Result<Self, Self::Error> {
        Ok(Fq(value.try_into()?))
    }
}

impl From<[u8; 32]> for Fq {
    fn from(value: [u8; 32]) -> Self {
        Fq(value)
    }
}

/// Error type for the alt_bn128 module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, BorshDeserialize, BorshSerialize)]
#[borsh(crate = "crate::serializers::borsh", use_discriminant = true)]
#[repr(u32)]
pub enum AltBn128Error {
    /// Invalid point x coordinate.
    InvalidXCoordinate = 1,
    /// Invalid point y coordinate.
    InvalidYCoordinate = 2,
    /// Invalid point.
    InvalidPoint = 3,
    /// Invalid A.
    InvalidA = 4,
    /// Invalid B.
    InvalidB = 5,
    /// Invalid Ax.
    InvalidAx = 6,
    /// Invalid Ay.
    InvalidAy = 7,
    /// Invalid Bay.
    InvalidBay = 8,
    /// Invalid Bax.
    InvalidBax = 9,
    /// Invalid Bby.
    InvalidBby = 10,
    /// Invalid Bbx.
    InvalidBbx = 11,
    /// No return value or error
    NoValueNorError = 12,
    /// Call error
    CallError = 13,
    /// Couldnt deserialize return value
    ReturnNotDeserializable = 14,
    /// Invalid length.
    InvalidLength = 15,
    /// Unknown error.
    Unknown(u32),
}

impl From<AltBn128Error> for u32 {
    fn from(value: AltBn128Error) -> Self {
        match value {
            AltBn128Error::InvalidXCoordinate => 1,
            AltBn128Error::InvalidYCoordinate => 2,
            AltBn128Error::InvalidPoint => 3,
            AltBn128Error::InvalidA => 4,
            AltBn128Error::InvalidB => 5,
            AltBn128Error::InvalidAx => 6,
            AltBn128Error::InvalidAy => 7,
            AltBn128Error::InvalidBay => 8,
            AltBn128Error::InvalidBax => 9,
            AltBn128Error::InvalidBby => 10,
            AltBn128Error::InvalidBbx => 11,
            AltBn128Error::NoValueNorError => 12,
            AltBn128Error::CallError => 13,
            AltBn128Error::ReturnNotDeserializable => 14,
            AltBn128Error::InvalidLength => 15,
            AltBn128Error::Unknown(catch_all) => catch_all,
        }
    }
}
impl From<u32> for AltBn128Error {
    fn from(value: u32) -> Self {
        match value {
            1 => AltBn128Error::InvalidXCoordinate,
            2 => AltBn128Error::InvalidYCoordinate,
            3 => AltBn128Error::InvalidPoint,
            4 => AltBn128Error::InvalidA,
            5 => AltBn128Error::InvalidB,
            6 => AltBn128Error::InvalidAx,
            7 => AltBn128Error::InvalidAy,
            8 => AltBn128Error::InvalidBay,
            9 => AltBn128Error::InvalidBax,
            10 => AltBn128Error::InvalidBby,
            11 => AltBn128Error::InvalidBbx,
            12 => AltBn128Error::NoValueNorError,
            13 => AltBn128Error::CallError,
            14 => AltBn128Error::ReturnNotDeserializable,
            15 => AltBn128Error::InvalidLength,
            value => AltBn128Error::Unknown(value),
        }
    }
}

/// Result type for the alt_bn128 module.
pub type Result<T> = core::result::Result<T, AltBn128Error>;

/// Adds two points on the alt_bn128 curve.
pub fn alt_bn128_add(x1: &G1, y1: &G1, x2: &G1, y2: &G1) -> Result<(Fq, Fq)> {
    let input = borsh::to_vec(&(x1, y1, x2, y2)).expect("Serialization to succeed");
    let option = CryptoFunctionOption::AltBn128Add;
    let (output, result) = casper_system(option.into(), &input);
    result.map_err(|_err| AltBn128Error::CallError)?;
    match output {
        Some(raw) => {
            let val: core::result::Result<([u8; 32], [u8; 32]), u32> =
                borsh::from_slice(&raw).map_err(|_err| AltBn128Error::ReturnNotDeserializable)?;
            val.map(|(x, y)| (Fq(x), Fq(y)))
                .map_err(AltBn128Error::from)
        }
        None => Err(AltBn128Error::NoValueNorError),
    }
}

/// Multiplies a point on the alt_bn128 curve by a scalar.
pub fn alt_bn128_mul(x: &G1, y: &G1, scalar: &Fr) -> Result<(Fq, Fq)> {
    let input = borsh::to_vec(&(x, y, scalar)).expect("Serialization to succeed");
    let option = CryptoFunctionOption::AltBn128Multiply;
    let (output, result) = casper_system(option.into(), &input);
    result.map_err(|_err| AltBn128Error::CallError)?;
    match output {
        Some(raw) => {
            let val: core::result::Result<([u8; 32], [u8; 32]), u32> =
                borsh::from_slice(&raw).map_err(|_err| AltBn128Error::ReturnNotDeserializable)?;
            val.map(|(x, y)| (Fq(x), Fq(y)))
                .map_err(AltBn128Error::from)
        }
        None => Err(AltBn128Error::NoValueNorError),
    }
}

/// A pairing of points on the alt_bn128 curve.
#[derive(Clone, BorshSerialize, BorshDeserialize, Debug)]
pub struct Pair {
    /// G1 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    ax: [u8; 32],
    /// G1 point y-coordinate. 32 bytes little-endian encoded unsigned integer
    ay: [u8; 32],
    /// Fq2 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    bax: [u8; 32],
    /// Fq2 point y-coordinate.32 bytes little-endian encoded unsigned integer
    bay: [u8; 32],
    /// Fq2 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    bbx: [u8; 32],
    /// Fq2 point x-coordinate. 32 bytes little-endian encoded unsigned integer
    bby: [u8; 32],
}

impl Pair {
    pub fn from(ax: Fq, ay: Fq, bax: Fq, bay: Fq, bbx: Fq, bby: Fq) -> Self {
        Pair {
            ax: ax.0,
            ay: ay.0,
            bax: bax.0,
            bay: bay.0,
            bbx: bbx.0,
            bby: bby.0,
        }
    }
    pub fn zero() -> Self {
        Pair {
            ax: [0; 32],
            ay: [0; 32],
            bax: [0; 32],
            bay: [0; 32],
            bbx: [0; 32],
            bby: [0; 32],
        }
    }
}

const _: () = assert!(
    core::mem::size_of::<Pair>() == 192,
    "Pair size is not correct",
);

/// Performs a pairing of points on the alt_bn128 curve.
pub fn alt_bn128_pairing(pairs: &[Pair]) -> Result<bool> {
    let input = borsh::to_vec(pairs).expect("Serialization to succeed");
    let option = CryptoFunctionOption::AltBn128Pairing;

    let (output, result) = casper_system(option.into(), &input);
    result.map_err(|_err| AltBn128Error::CallError)?;
    match output {
        Some(raw) => {
            let val: core::result::Result<bool, u32> =
                borsh::from_slice(&raw).map_err(|_err| AltBn128Error::ReturnNotDeserializable)?;
            val.map_err(AltBn128Error::from)
        }
        None => Err(AltBn128Error::NoValueNorError),
    }
}

fn u256_to_le_bytes(value: U256) -> [u8; 32] {
    // For some reason bnum::to_le_bytes is behind "nightly" flag,
    // this is a helper method o achieve the same functionality.
    // Should be replaced with bnum functionality once it evolves past nightly
    let digits = value.digits();
    let mut bytes = [0; 32];
    let mut i = 0;
    while i < 4 {
        let digit_bytes = digits[i].to_le_bytes();
        let mut j = 0;
        while j < 8 {
            bytes[(i << 3) + j] = digit_bytes[j];
            j += 1;
        }
        i += 1;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use crate::casper::altbn128::u256_to_le_bytes;
    use bnum::types::U256;
    use casper_types::{testing::TestRng, U256 as CasperU256};
    use rand::Rng;

    #[test]
    fn test_u256_to_le_bytes() {
        let mut test_rng = TestRng::new();
        for _ in 0..50 {
            let u: CasperU256 = test_rng.gen();
            let base_10_str = format!("{}", u);
            let mut expected_le_bytes = [0; 32];
            u.to_little_endian(&mut expected_le_bytes);
            let u256 = U256::parse_str_radix(&base_10_str, 10);
            let bytes = u256_to_le_bytes(u256);
            assert_eq!(bytes, expected_le_bytes);
        }
    }
}
