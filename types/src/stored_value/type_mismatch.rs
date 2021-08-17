use alloc::string::String;
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
use thiserror::Error;

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "std", derive(Error))]
#[cfg_attr(
    feature = "std",
    error("Type mismatch. Expected {expected} but found {found}.")
)]
/// An error struct representing a type mismatch in [`StoredValue`] operations.
pub struct TypeMismatch {
    /// The name of the expected type.
    pub expected: String,
    /// The actual type found.
    pub found: String,
}

impl TypeMismatch {
    /// Creates a new `TypeMismatch`.
    pub fn new(expected: String, found: String) -> TypeMismatch {
        TypeMismatch { expected, found }
    }
}
