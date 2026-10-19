#![no_std]
#![no_main]

const NODE_02_ADDR: &[u8; 64] = b"4ee7ad9b21fd625481d0a94c618a15ab92503a7457e428a4dcd9dd6f100e979b";
const INITIAL_AMOUNT: u64 = 1_000_000;

#[no_mangle]
pub extern "C" fn call() {
    create_test_node_shared::create_account(NODE_02_ADDR, INITIAL_AMOUNT)
}
