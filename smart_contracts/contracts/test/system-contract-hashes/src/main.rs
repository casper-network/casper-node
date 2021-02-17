#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::system::{AUCTION, MINT, PROOF_OF_STAKE};

#[no_mangle]
pub extern "C" fn call() {
    runtime::put_key(MINT, system::get_mint().into());
    runtime::put_key(PROOF_OF_STAKE, system::get_proof_of_stake().into());
    runtime::put_key(AUCTION, system::get_auction().into());
}
