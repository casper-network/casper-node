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
/// TODO: doc comment.
pub struct TypeMismatch {
    /// TODO: doc comment.
    pub expected: String,
    /// TODO: doc comment.
    pub found: String,
}

impl TypeMismatch {
    /// TODO: doc comment.
    pub fn new(expected: String, found: String) -> TypeMismatch {
        TypeMismatch { expected, found }
    }
}
