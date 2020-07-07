#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};

use contract::{contract_api::storage, unwrap_or_revert::UnwrapOrRevert};

pub const LOCAL_KEY: [u8; 32] = [66u8; 32];
pub const HELLO_PREFIX: &str = " Hello, ";
pub const WORLD_SUFFIX: &str = "world!";

pub fn delegate() {
    // Appends " Hello, world!" to a [66; 32] local key with spaces trimmed.
    // Two runs should yield value "Hello, world! Hello, world!"
    // read from local state
    let mut res: String = storage::read_local(&LOCAL_KEY)
        .unwrap_or_default()
        .unwrap_or_default();

    res.push_str(HELLO_PREFIX);
    // Write "Hello, "
    storage::write_local(LOCAL_KEY, res);

    // Read (this should exercise cache)
    let mut res: String = storage::read_local(&LOCAL_KEY)
        .unwrap_or_revert()
        .unwrap_or_revert();
    // Append
    res.push_str(WORLD_SUFFIX);
    // Write
    storage::write_local(LOCAL_KEY, res.trim().to_string());
}
