//! Contains implementation of a Handle Payment contract functionality.
mod constants;
mod entry_points;
mod error;

pub use constants::*;
pub use entry_points::handle_payment_entry_points;
pub use error::Error;
