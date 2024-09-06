//! Built-in functions provided by the Casper runtime.
//!
//! These functions are lifting otherwise very expensive operations to the host.
use casper_types::ApiError;

use crate::ext_ffi;

/// A point on the alt_bn128 curve.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
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

impl TryFrom<&[u8]> for G1 {
    type Error = ApiError;

    fn try_from(value: &[u8]) -> core::result::Result<Self, Self::Error> {
        Ok(G1(value
            .try_into()
            .map_err(|_| ApiError::BufferTooSmall)?))
    }
}

/// Error type for the alt_bn128 module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Error {
    /// Invalid x coordinate.
    InvalidXCoordinate = 1,
    /// Invalid y coordinate.
    InvalidYCoordinate = 2,
    /// Invalid point.
    InvalidPoint = 3,
    /// Unknown error.
    Unknown(i32),
}

impl From<i32> for Error {
    fn from(value: i32) -> Self {
        match value {
            1 => Error::InvalidXCoordinate,
            2 => Error::InvalidYCoordinate,
            3 => Error::InvalidPoint,
            value => Error::Unknown(value),
        }
    }
}

/// Result type for the alt_bn128 module.
pub type Result<T> = core::result::Result<T, Error>;

/// Adds two points on the alt_bn128 curve.
pub fn alt_bn128_add(x1: &G1, y1: &G1, x2: &G1, y2: &G1) -> Result<(G1, G1)> {
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
        Ok((G1(x_buf), G1(y_buf)))
    } else {
        Err(Error::from(ret))
    }
}
