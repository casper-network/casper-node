//! This module provides types primarily to support converting instances of `BTreeMap<K, V>` into
//! `Vec<(K, V)>` or similar, in order to allow these types to be able to be converted to and from
//! JSON, and to allow for the production of a static schema for them.

#![cfg(all(feature = "std", feature = "json-schema"))]
mod json_block_with_signatures;

pub use json_block_with_signatures::JsonBlockWithSignatures;
