#![no_std]
#![no_main]

use casper_contract::contract_api::system;
use casper_types::ContractHash;

const MINT_CONTRACT_HASH: ContractHash = ContractHash::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]);
const AUCTION_CONTRACT_HASH: ContractHash = ContractHash::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
]);
const HANDLE_PAYMENT_CONTRACT_HASH: ContractHash = ContractHash::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3,
]);
const STANDARD_PAYMENT_CONTRACT_HASH: ContractHash = ContractHash::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4,
]);

#[no_mangle]
pub extern "C" fn call() {
    assert_eq!(MINT_CONTRACT_HASH, system::get_mint(),);
    assert_eq!(AUCTION_CONTRACT_HASH, system::get_auction(),);
    assert_eq!(HANDLE_PAYMENT_CONTRACT_HASH, system::get_handle_payment(),);
    assert_eq!(
        STANDARD_PAYMENT_CONTRACT_HASH,
        system::get_standard_payment(),
    );
}
