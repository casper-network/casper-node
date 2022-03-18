#![allow(clippy::return_self_not_must_use)]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

#[cfg(not(any(feature = "std", test)))]
extern crate alloc;

pub mod capnp;
mod global_state_capnp;
pub mod map;
