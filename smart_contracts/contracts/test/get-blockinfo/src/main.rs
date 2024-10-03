#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, runtime::revert},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    bytesrepr::{Bytes, FromBytes},
    ApiError, BlockTime, Digest,
};

const ARG_FIELD_IDX: &str = "field_idx";
const FIELD_IDX_BLOCK_TIME: u8 = 0;
const FIELD_IDX_BLOCK_HEIGHT: u8 = 1;
const FIELD_IDX_PARENT_BLOCK_HASH: u8 = 2;
const FIELD_IDX_STATE_HASH: u8 = 3;

const CURRENT_UBOUND: u8 = FIELD_IDX_STATE_HASH;
const ARG_KNOWN_BLOCK_TIME: &str = "known_block_time";
const ARG_KNOWN_BLOCK_HEIGHT: &str = "known_block_height";
const ARG_KNOWN_BLOCK_PARENT_HASH: &str = "known_block_parent_hash";
const ARG_KNOWN_STATE_HASH: &str = "known_state_hash";

#[no_mangle]
pub extern "C" fn call() {
    let field_idx: u8 = runtime::get_named_arg(ARG_FIELD_IDX);
    if field_idx > CURRENT_UBOUND {
        revert(ApiError::Unhandled);
    }
    if field_idx == FIELD_IDX_BLOCK_TIME {
        let expected = BlockTime::new(runtime::get_named_arg(ARG_KNOWN_BLOCK_TIME));
        let actual: BlockTime = runtime::get_blocktime();
        if expected != actual {
            revert(ApiError::User(field_idx as u16));
        }
    }
    if field_idx == FIELD_IDX_BLOCK_HEIGHT {
        let expected: u64 = runtime::get_named_arg(ARG_KNOWN_BLOCK_HEIGHT);
        let actual = runtime::get_block_height();
        if expected != actual {
            revert(ApiError::User(field_idx as u16));
        }
    }
    if field_idx == FIELD_IDX_PARENT_BLOCK_HASH {
        let bytes: Bytes = runtime::get_named_arg(ARG_KNOWN_BLOCK_PARENT_HASH);
        let (expected, _rem) = Digest::from_bytes(bytes.inner_bytes())
            .unwrap_or_revert_with(ApiError::User(CURRENT_UBOUND as u16 + 1));
        let actual = runtime::get_parent_block_hash();
        if expected != actual {
            revert(ApiError::User(field_idx as u16));
        }
    }
    if field_idx == FIELD_IDX_STATE_HASH {
        let bytes: Bytes = runtime::get_named_arg(ARG_KNOWN_STATE_HASH);
        let (expected, _rem) = Digest::from_bytes(bytes.inner_bytes())
            .unwrap_or_revert_with(ApiError::User(CURRENT_UBOUND as u16 + 2));
        let actual = runtime::get_state_hash();
        if expected != actual {
            revert(ApiError::User(field_idx as u16));
        }
    }
}
