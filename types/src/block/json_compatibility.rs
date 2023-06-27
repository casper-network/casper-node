//! This module provides types primarily to support converting instances of `BTreeMap<K, V>` into
//! `Vec<(K, V)>` or similar, in order to allow these types to be able to be converted to and from
//! JSON, and to allow for the production of a static schema for them.

#![cfg(all(feature = "std", feature = "json-schema"))]

mod json_block;
mod json_block_body;
mod json_block_header;
mod json_era_end;
mod json_era_report;
mod json_proof;
mod json_reward;
mod json_validator_weight;

pub use json_block::JsonBlock;
pub use json_block_body::JsonBlockBody;
pub use json_block_header::JsonBlockHeader;
pub use json_era_end::JsonEraEnd;
pub use json_era_report::JsonEraReport;
pub use json_proof::JsonProof;
pub use json_reward::JsonReward;
pub use json_validator_weight::JsonValidatorWeight;
