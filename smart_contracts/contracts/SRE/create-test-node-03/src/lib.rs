#![no_std]
#![no_main]

const NODE_03_ADDR: &[u8; 64] = b"a3b2fd2971f2de5145d2342df38555ce97070a27ef7e74b63e08c482697308dd";
const INITIAL_AMOUNT: u64 = 1_000_000;

#[no_mangle]
pub extern "C" fn call() {
    create_test_node_shared::create_account(NODE_03_ADDR, INITIAL_AMOUNT)
}
