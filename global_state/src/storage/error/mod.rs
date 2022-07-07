//! Storage errors.

/// Errors for LMDB storage implementation.
pub mod lmdb;

/// Public re-export of `lmdb` crate's `Error` type.
pub use self::lmdb::Error;
