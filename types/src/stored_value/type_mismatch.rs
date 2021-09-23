use alloc::string::String;
use core::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
/// An error struct representing a type mismatch in [`StoredValue`](crate::StoredValue) operations.
pub struct TypeMismatch {
    /// The name of the expected type.
    expected: String,
    /// The actual type found.
    found: String,
}

impl TypeMismatch {
    /// Creates a new `TypeMismatch`.
    pub fn new(expected: String, found: String) -> TypeMismatch {
        TypeMismatch { expected, found }
    }
}

impl Display for TypeMismatch {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "Type mismatch. Expected {} but found {}.",
            self.expected, self.found
        )
    }
}
