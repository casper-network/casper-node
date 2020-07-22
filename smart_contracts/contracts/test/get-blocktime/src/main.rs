#![no_std]
#![no_main]

use casperlabs_contract::contract_api::runtime;
use casperlabs_types::BlockTime;

const ARG_KNOWN_BLOCK_TIME: &str = "known_block_time";

#[no_mangle]
pub extern "C" fn call() {
    let known_block_time: u64 = runtime::get_named_arg(ARG_KNOWN_BLOCK_TIME);
    let actual_block_time: BlockTime = runtime::get_blocktime();

    assert_eq!(
        actual_block_time,
        BlockTime::new(known_block_time),
        "actual block time not known block time"
    );
}
