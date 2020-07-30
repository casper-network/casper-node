#![cfg_attr(not(test), no_std)]

#[macro_use]
extern crate alloc;

mod entry_points;

pub use entry_points::get_entry_points;
