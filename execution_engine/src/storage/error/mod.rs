//! Storage errors.

/// Errors for DB storage implementation.
pub mod db;
/// Errors for In-Memory storage implementation.
pub mod in_memory;

/// Public re-export of `lmdb` crate's `Error` type.
pub use self::db::Error;
