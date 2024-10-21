#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;

use casper_contract::contract_api::{cryptography, runtime};
use casper_types::crypto::HashAlgorithm;

const ARG_ALGORITHM: &str = "algorithm";
const ARG_DATA: &str = "data";
const ARG_EXPECTED: &str = "expected";

#[no_mangle]
pub extern "C" fn call() {
    let data: String = runtime::get_named_arg(ARG_DATA);
    let expected: [u8; 32] = runtime::get_named_arg(ARG_EXPECTED);
    let algorithm_repr: u8 = runtime::get_named_arg(ARG_ALGORITHM);

    let algorithm = HashAlgorithm::try_from(algorithm_repr).expect("Invalid enum repr");
    let hash = cryptography::generic_hash(data, algorithm);

    assert_eq!(hash, expected, "Hash mismatch");
}
