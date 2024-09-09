//! Host-optimized support for pairing cryptography with the Barreto-Naehrig curve
use core::{array::TryFromSliceError, ffi::c_void, mem::MaybeUninit};

use bytemuck::{Pod, Zeroable};
use casper_types::U256;

use crate::ext_ffi;

/// A point on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(C, packed)]
pub struct G1([u8; 32]);

impl G1 {
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
        let bytes = value.to_le_bytes();
        G1(bytes)
    }
}

/// A scalar on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(C, packed)]
pub struct Fr([u8; 32]);

impl Fr {
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
        let bytes = value.to_le_bytes();
        Self(bytes)
    }
}

/// A field element on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Zeroable, Pod)]
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
        let bytes = value.to_le_bytes();
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for Fq {
    type Error = TryFromSliceError;

    fn try_from(value: &[u8]) -> core::result::Result<Self, Self::Error> {
        Ok(Fq(value.try_into()?))
    }
}

/// Error type for the alt_bn128 module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Error {
    /// Invalid length.
    InvalidLength = 1,
    /// Invalid point x coordinate.
    InvalidXCoordinate = 2,
    /// Invalid point y coordinate.
    InvalidYCoordinate = 3,
    /// Invalid point.
    InvalidPoint = 4,
    /// Invalid A.
    InvalidA = 5,
    /// Invalid B.
    InvalidB = 6,
    /// Invalid Ax.
    InvalidAx = 7,
    /// Invalid Ay.
    InvalidAy = 8,
    /// Invalid Bay.
    InvalidBay = 9,
    /// Invalid Bax.
    InvalidBax = 10,
    /// Invalid Bby.
    InvalidBby = 11,
    /// Invalid Bbx.
    InvalidBbx = 12,
    /// Unknown error.
    Unknown(i32),
}

impl From<i32> for Error {
    fn from(value: i32) -> Self {
        match value {
            1 => Error::InvalidLength,
            2 => Error::InvalidXCoordinate,
            3 => Error::InvalidYCoordinate,
            4 => Error::InvalidPoint,
            5 => Error::InvalidA,
            6 => Error::InvalidB,
            7 => Error::InvalidAx,
            8 => Error::InvalidAy,
            9 => Error::InvalidBay,
            10 => Error::InvalidBax,
            11 => Error::InvalidBby,
            12 => Error::InvalidBbx,
            value => Error::Unknown(value),
        }
    }
}

/// Result type for the alt_bn128 module.
pub type Result<T> = core::result::Result<T, Error>;

/// Adds two points on the alt_bn128 curve.
pub fn alt_bn128_add(x1: &G1, y1: &G1, x2: &G1, y2: &G1) -> Result<(Fq, Fq)> {
    let mut x_buf = [0; 32];
    let mut y_buf = [0; 32];

    let ret = unsafe {
        ext_ffi::casper_alt_bn128_add(
            x1.0.as_ptr(),
            y1.0.as_ptr(),
            x2.0.as_ptr(),
            y2.0.as_ptr(),
            x_buf.as_mut_ptr(),
            y_buf.as_mut_ptr(),
        )
    };

    if ret == 0 {
        Ok((Fq(x_buf), Fq(y_buf)))
    } else {
        Err(Error::from(ret))
    }
}

/// Multiplies a point on the alt_bn128 curve by a scalar.
pub fn alt_bn128_mul(x: &G1, y: &G1, scalar: &Fr) -> Result<(Fq, Fq)> {
    let mut x_buf = [0; 32];
    let mut y_buf = [0; 32];

    let ret = unsafe {
        ext_ffi::casper_alt_bn128_mul(
            x.0.as_ptr(),
            y.0.as_ptr(),
            scalar.0.as_ptr(),
            x_buf.as_mut_ptr(),
            y_buf.as_mut_ptr(),
        )
    };

    if ret == 0 {
        Ok((Fq(x_buf), Fq(y_buf)))
    } else {
        Err(Error::from(ret))
    }
}

/// A pairing of points on the alt_bn128 curve.
#[derive(Copy, Clone, Zeroable, Pod)]
#[repr(C, packed)]
pub struct Pair {
    /// G1 point
    pub ax: Fq,
    /// G1 point
    pub ay: Fq,
    /// G2 point
    pub bax: Fq,
    /// G2 point
    pub bay: Fq,
    /// G1 point
    pub bbx: Fq,
    /// G1 point
    pub bby: Fq,
}

const _: () = assert!(
    core::mem::size_of::<Pair>() == 192,
    "Pair size is not correct",
);

/// Performs a pairing of points on the alt_bn128 curve.
pub fn alt_bn128_pairing(points: &[Pair]) -> Result<bool> {
    let mut result = MaybeUninit::uninit();

    let bytes: &[u8] = bytemuck::cast_slice(points);

    let ret = unsafe {
        ext_ffi::casper_alt_bn128_pairing(
            bytes.as_ptr() as *const c_void,
            bytes.len(),
            result.as_mut_ptr(),
        )
    };

    if ret == 0 {
        Ok(unsafe { result.assume_init() } != 0)
    } else {
        Err(Error::from(ret))
    }
}
