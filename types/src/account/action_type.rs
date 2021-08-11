use core::convert::TryFrom;

use super::TryFromIntError;

/// The various types of action which can be performed in the context of a given account.
#[repr(u32)]
pub enum ActionType {
    /// Represents performing a deploy.
    Deployment = 0,
    /// Represents changing the associated keys (i.e. map of [`AccountHash`](super::AccountHash)s
    /// to [`Weight`](super::Weight)s) or action thresholds (i.e. the total
    /// [`Weight`](super::Weight)s of signing [`AccountHash`](super::AccountHash)s required to
    /// perform various actions).
    KeyManagement = 1,
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<u32> for ActionType {
    type Error = TryFromIntError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        // This doesn't use `num_derive` traits such as FromPrimitive and ToPrimitive
        // that helps to automatically create `from_u32` and `to_u32`. This approach
        // gives better control over generated code.
        match value {
            d if d == ActionType::Deployment as u32 => Ok(ActionType::Deployment),
            d if d == ActionType::KeyManagement as u32 => Ok(ActionType::KeyManagement),
            _ => Err(TryFromIntError(())),
        }
    }
}
