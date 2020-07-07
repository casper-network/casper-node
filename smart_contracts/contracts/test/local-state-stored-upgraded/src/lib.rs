#![no_std]

extern crate alloc;

use alloc::string::String;

use contract::{contract_api::storage, unwrap_or_revert::UnwrapOrRevert};
use types::ApiError;

pub const ENTRY_FUNCTION_NAME: &str = "delegate";
pub const CONTRACT_NAME: &str = "local_state_stored";
pub const SNIPPET: &str = " I've been upgraded!";

#[repr(u16)]
enum CustomError {
    LocalKeyReadMutatedBytesRepr = 1,
    UnableToReadMutatedLocalKey = 2,
}

pub fn delegate() {
    local_state::delegate();
    // read from local state
    let mut res: String = storage::read_local(&local_state::LOCAL_KEY)
        .unwrap_or_default()
        .unwrap_or_default();

    res.push_str(SNIPPET);
    // Write "Hello, "
    storage::write_local(local_state::LOCAL_KEY, res);

    // Read back
    let res: String = storage::read_local(&local_state::LOCAL_KEY)
        .unwrap_or_revert_with(ApiError::User(
            CustomError::UnableToReadMutatedLocalKey as u16,
        ))
        .unwrap_or_revert_with(ApiError::User(
            CustomError::LocalKeyReadMutatedBytesRepr as u16,
        ));

    // local state should be available after upgrade
    assert!(
        !res.is_empty(),
        "local value should be accessible post upgrade"
    )
}
