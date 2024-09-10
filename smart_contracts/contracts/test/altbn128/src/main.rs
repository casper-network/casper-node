#![no_std]
#![no_main]

extern crate alloc;

use core::{ffi::c_void, mem::MaybeUninit};

use alloc::format;
use casper_contract::{
    contract_api::{
        builtins::altbn128::{self, Error, Fq, Fr, Pair, G1},
        runtime,
    },
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, U256};

fn alt_bn128_pairing_raw(input: &[u8]) -> altbn128::Result<bool> {
    let mut result = MaybeUninit::uninit();

    let ret = unsafe {
        ext_ffi::casper_alt_bn128_pairing(
            input.as_ptr() as *const c_void,
            input.len(),
            result.as_mut_ptr(),
        )
    };

    if ret == 0 {
        Ok(unsafe { result.assume_init() } != 0)
    } else {
        Err(Error::from(ret))
    }
}

fn test_alt_bn128_add() {
    let x1: G1 = U256::from_str_radix(
        "18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9",
        16,
    )
    .ok()
    .unwrap_or_revert_with(ApiError::User(line!() as _))
    .into();

    let y1 = U256::from_str_radix(
        "063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266",
        16,
    )
    .ok()
    .unwrap_or_revert_with(ApiError::User(line!() as _))
    .into();

    let x2 = U256::from_str_radix(
        "07c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed",
        16,
    )
    .ok()
    .unwrap_or_revert_with(ApiError::User(line!() as _))
    .into();
    let y2 = U256::from_str_radix(
        "06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7",
        16,
    )
    .ok()
    .unwrap_or_revert_with(ApiError::User(line!() as _))
    .into();

    let expected_x = U256::from_str_radix(
        "2243525c5efd4b9c3d3c45ac0ca3fe4dd85e830a4ce6b65fa1eeaee202839703",
        16,
    )
    .ok()
    .unwrap_or_revert_with(ApiError::User(line!() as _))
    .into();

    let expected_y = U256::from_str_radix(
        "301d1d33be6da8e509df21cc35964723180eed7532537db9ae5e7d48f195c915",
        16,
    )
    .ok()
    .unwrap_or_revert_with(ApiError::User(line!() as _))
    .into();

    let result = altbn128::alt_bn128_add(&x1, &y1, &x2, &y2);
    let actual = Ok((expected_x, expected_y));
    if result != actual {
        runtime::print(&format!("left {:?} right {:?}", result, actual));
        runtime::revert(ApiError::User(line!() as _));
    }
}

fn test_zero() {
    if altbn128::alt_bn128_add(&G1::zero(), &G1::zero(), &G1::zero(), &G1::zero())
        != Ok((Fq::zero(), Fq::zero()))
    {
        runtime::revert(ApiError::User(line!() as _));
    }
}

fn test_add_error() {
    let all_ones = U256::from_str_radix(
        "1111111111111111111111111111111111111111111111111111111111111111",
        16,
    )
    .ok()
    .map(G1::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));

    if altbn128::alt_bn128_add(&all_ones, &all_ones, &all_ones, &all_ones)
        != Err(Error::InvalidPoint)
    {
        runtime::revert(ApiError::User(line!() as _));
    }
}

fn test_alt_bn128_mul() {
    let x = U256::from_str_radix(
        "2bd3e6d0f3b142924f5ca7b49ce5b9d54c4703d7ae5648e61d02268b1a0a9fb7",
        16,
    )
    .map(G1::from)
    .unwrap();
    let y = U256::from_str_radix(
        "21611ce0a6af85915e2f1d70300909ce2e49dfad4a4619c8390cae66cefdb204",
        16,
    )
    .map(G1::from)
    .unwrap();
    let scalar = U256::from_str_radix(
        "00000000000000000000000000000000000000000000000011138ce750fa15c2",
        16,
    )
    .map(Fr::from)
    .unwrap();

    let expected_x = U256::from_str_radix(
        "070a8d6a982153cae4be29d434e8faef8a47b274a053f5a4ee2a6c9c13c31e5c",
        16,
    )
    .map(Fq::from)
    .unwrap();
    let expected_y = U256::from_str_radix(
        "031b8ce914eba3a9ffb989f9cdd5b0f01943074bf4f0f315690ec3cec6981afc",
        16,
    )
    .map(Fq::from)
    .unwrap();

    assert_eq!(
        altbn128::alt_bn128_mul(&x, &y, &scalar),
        Ok((expected_x, expected_y))
    );
}

fn test_zero_multiplication() {
    // zero multiplication test

    let x = U256::from_str_radix(
        "0000000000000000000000000000000000000000000000000000000000000000",
        16,
    )
    .ok()
    .map(G1::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let y = U256::from_str_radix(
        "0000000000000000000000000000000000000000000000000000000000000000",
        16,
    )
    .ok()
    .map(G1::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let scalar = U256::from_str_radix(
        "0200000000000000000000000000000000000000000000000000000000000000",
        16,
    )
    .ok()
    .map(Fr::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));

    let expected_x = U256::from_str_radix(
        "0000000000000000000000000000000000000000000000000000000000000000",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let expected_y = U256::from_str_radix(
        "0000000000000000000000000000000000000000000000000000000000000000",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));

    assert_eq!(
        altbn128::alt_bn128_mul(&x, &y, &scalar),
        Ok((expected_x, expected_y))
    );
}

fn test_not_on_curve_multiplication() {
    // point not on curve fail

    let x = U256::from_str_radix(
        "1111111111111111111111111111111111111111111111111111111111111111",
        16,
    )
    .ok()
    .map(G1::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let y = U256::from_str_radix(
        "1111111111111111111111111111111111111111111111111111111111111111",
        16,
    )
    .ok()
    .map(G1::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let scalar = U256::from_str_radix(
        "0f00000000000000000000000000000000000000000000000000000000000000",
        16,
    )
    .ok()
    .map(Fr::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    assert_eq!(
        altbn128::alt_bn128_mul(&x, &y, &scalar),
        Err(Error::InvalidPoint)
    );
}

fn test_pairing() {
    let ax_1 = U256::from_str_radix(
        "1c76476f4def4bb94541d57ebba1193381ffa7aa76ada664dd31c16024c43f59",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let ay_1 = U256::from_str_radix(
        "3034dd2920f673e204fee2811c678745fc819b55d3e9d294e45c9b03a76aef41",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bax_1 = U256::from_str_radix(
        "209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf7",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bay_1 = U256::from_str_radix(
        "04bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a41678",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bbx_1 = U256::from_str_radix(
        "2bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bby_1 = U256::from_str_radix(
        "120a2a4cf30c1bf9845f20c6fe39e07ea2cce61f0c9bb048165fe5e4de877550",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let ax_2 = U256::from_str_radix(
        "111e129f1cf1097710d41c4ac70fcdfa5ba2023c6ff1cbeac322de49d1b6df7c",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let ay_2 = U256::from_str_radix(
        "2032c61a830e3c17286de9462bf242fca2883585b93870a73853face6a6bf411",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bax_2 = U256::from_str_radix(
        "198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c2",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bay_2 = U256::from_str_radix(
        "1800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bbx_2 = U256::from_str_radix(
        "090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));
    let bby_2 = U256::from_str_radix(
        "12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
        16,
    )
    .ok()
    .map(Fq::from)
    .unwrap_or_revert_with(ApiError::User(line!() as _));

    let all_pairs = [
        Pair {
            ax: ax_1,
            ay: ay_1,
            bax: bax_1,
            bay: bay_1,
            bbx: bbx_1,
            bby: bby_1,
        },
        Pair {
            ax: ax_2,
            ay: ay_2,
            bax: bax_2,
            bay: bay_2,
            bbx: bbx_2,
            bby: bby_2,
        },
    ];

    assert_eq!(altbn128::alt_bn128_pairing(&all_pairs), Ok(true));

    assert_eq!(alt_bn128_pairing_raw(&[0u8; 0]), Ok(true));
    assert_eq!(alt_bn128_pairing_raw(&[0u8; 1]), Err(Error::InvalidLength));
    assert_eq!(
        unsafe {
            let mut result = MaybeUninit::uninit();
            ext_ffi::casper_alt_bn128_pairing(
                [0u8; 0].as_ptr() as *const c_void,
                193,
                result.as_mut_ptr(),
            )
        },
        1
    );
}

fn test_pairing_no_input() {
    assert_eq!(alt_bn128_pairing_raw(&[]), Ok(true));
}

fn test_pairing_invalid_a() {
    let pair = Pair {
        ax: Fq::from([0x11u8; 32]),
        ay: Fq::from([0x11u8; 32]),
        bax: Fq::from([0x11u8; 32]),
        bay: Fq::from([0x11u8; 32]),
        bbx: Fq::from([0x11u8; 32]),
        bby: Fq::from([0x11u8; 32]),
    };

    assert_eq!(altbn128::alt_bn128_pairing(&[pair]), Err(Error::InvalidA));
}

fn test_pairing_on_curve() {
    let pair = Pair {
        ax: Fq::from([0; 32]),
        ay: Fq::from([0; 32]),
        bax: Fq::from([0; 32]),
        bay: Fq::from([0; 32]),
        bbx: Fq::from([0; 32]),
        bby: Fq::from([0; 32]),
    };

    assert_eq!(altbn128::alt_bn128_pairing(&[pair]), Ok(true),);
}
#[no_mangle]
pub extern "C" fn call() {
    test_alt_bn128_add();
    test_zero();
    test_add_error();
    test_alt_bn128_mul();
    test_zero_multiplication();
    test_not_on_curve_multiplication();
    test_pairing();
    test_pairing_no_input();
    test_pairing_invalid_a();
    test_pairing_on_curve();
}
