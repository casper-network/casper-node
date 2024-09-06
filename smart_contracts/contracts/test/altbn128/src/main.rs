#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use casper_contract::{
    contract_api::{
        builtins::{self, Error, G1},
        runtime,
    },
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::ApiError;

fn test_alt_bn128_add() {
    let x1: G1 = base16::decode("18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9")
        .ok()
        .unwrap_or_revert_with(ApiError::User(1))
        .as_slice()
        .try_into()
        .unwrap_or_revert_with(ApiError::User(2));

    let y1 = base16::decode("063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266")
        .ok()
        .unwrap_or_revert_with(ApiError::User(3))
        .as_slice()
        .try_into()
        .unwrap_or_revert_with(ApiError::User(4));

    let x2 = base16::decode("07c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed")
        .ok()
        .unwrap_or_revert_with(ApiError::User(5))
        .as_slice()
        .try_into()
        .unwrap_or_revert_with(ApiError::User(6));
    let y2 = base16::decode("06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7")
        .ok()
        .unwrap_or_revert_with(ApiError::User(7))
        .as_slice()
        .try_into()
        .unwrap_or_revert_with(ApiError::User(8));

    let expected_x =
        base16::decode("2243525c5efd4b9c3d3c45ac0ca3fe4dd85e830a4ce6b65fa1eeaee202839703")
            .ok()
            .unwrap_or_revert_with(ApiError::User(9))
            .as_slice()
            .try_into()
            .unwrap_or_revert_with(ApiError::User(10));
    let expected_y =
        base16::decode("301d1d33be6da8e509df21cc35964723180eed7532537db9ae5e7d48f195c915")
            .ok()
            .unwrap_or_revert_with(ApiError::User(11))
            .as_slice()
            .try_into()
            .unwrap_or_revert_with(ApiError::User(12));

    let result = builtins::alt_bn128_add(&x1, &y1, &x2, &y2);
    let actual = Ok((expected_x, expected_y));
    if result != actual {
        runtime::print(&format!("left {:?} right {:?}", result, actual));
        runtime::revert(ApiError::User(13));
    }
}

fn test_zero() {
    if builtins::alt_bn128_add(&G1::zero(), &G1::zero(), &G1::zero(), &G1::zero())
        != Ok((G1::zero(), G1::zero()))
    {
        runtime::revert(ApiError::User(14));
    }
}

fn test_add_error() {
    let all_ones =
        base16::decode("1111111111111111111111111111111111111111111111111111111111111111")
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap();

    if builtins::alt_bn128_add(&all_ones, &all_ones, &all_ones, &all_ones)
        != Err(Error::InvalidPoint)
    {
        runtime::revert(ApiError::User(15));
    }
}

#[no_mangle]
pub extern "C" fn call() {
    test_alt_bn128_add();
    test_zero();
    test_add_error();
}
