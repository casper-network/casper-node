//! Types for the `State::AllValues` request.

use alloc::vec::Vec;

use crate::StoredValue;

/// Represents a result of a `get_all_values` request.
#[derive(Debug, PartialEq)]
pub enum GetAllValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// Current values.
        values: Vec<StoredValue>,
    },
}
