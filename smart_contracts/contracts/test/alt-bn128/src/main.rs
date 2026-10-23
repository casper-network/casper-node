#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use casper_contract::contract_api::{
    cryptography::{self, AltBn128Coordinate},
    runtime,
};
use casper_types::{bytesrepr::Bytes, ApiError};

#[no_mangle]
pub extern "C" fn call() {
    let operation: u8 = runtime::get_named_arg("operation");
    let input: Vec<AltBn128Coordinate> = runtime::get_named_arg("input");
    let expected: Bytes = runtime::get_named_arg("expected");
    let expected_error: Option<u32> = runtime::get_named_arg("expected_error");

    let result: Result<Vec<u8>, ApiError> = match operation {
        0 => cryptography::alt_bn128_add(&input[0], &input[1], &input[2], &input[3])
            .map(|point| point.into_iter().flatten().collect()),
        1 => cryptography::alt_bn128_mul(&input[0], &input[1], &input[2])
            .map(|point| point.into_iter().flatten().collect()),
        2 => {
            let (pairs, remainder) = input.as_chunks::<6>();
            assert!(remainder.is_empty());
            cryptography::alt_bn128_pairing(pairs).map(|value| alloc::vec![u8::from(value)])
        }
        _ => runtime::revert(ApiError::InvalidArgument),
    };
    match expected_error {
        Some(error) => assert_eq!(result, Err(ApiError::from(error))),
        None => assert_eq!(result.unwrap().as_slice(), expected.as_ref()),
    }
}
