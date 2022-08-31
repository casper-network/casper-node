//! Storage errors.

/// Errors for In-Memory storage implementation.
pub mod in_memory;
/// Errors for DB storage implementation.
pub mod lmdb;

/// Public re-export of `lmdb` crate's `Error` type.
pub use self::lmdb::Error;
