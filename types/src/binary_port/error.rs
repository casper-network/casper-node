//! Binary port error.

#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
#[repr(u8)]
pub enum Error {
    #[cfg_attr(feature = "std", error("request executed correctly"))]
    NoError = 0,
    #[cfg_attr(feature = "std", error("data not found"))]
    NotFound = 1,
}
