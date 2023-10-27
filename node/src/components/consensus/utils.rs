//! Various utilities relevant to consensus.

mod validators;
mod weight;

pub use validators::{Validator, ValidatorIndex, ValidatorMap, Validators};
pub use weight::Weight;
