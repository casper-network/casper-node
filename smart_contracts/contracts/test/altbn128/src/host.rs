use alloc::format;
use core::{ffi::c_void, mem::MaybeUninit};

use casper_contract::{
    contract_api::{
        builtins::altbn128::{self, Error, Fq, Fr, Pair, G1},
        runtime,
    },
    ext_ffi,
};
use casper_types::ApiError;

const ADD_X1_LE: [u8; 32] = [
    201, 61, 9, 78, 3, 116, 59, 93, 12, 97, 145, 70, 18, 221, 17, 179, 133, 113, 142, 54, 17, 84,
    219, 118, 2, 195, 194, 180, 207, 138, 177, 24,
];
const ADD_Y1_LE: [u8; 32] = [
    102, 114, 243, 152, 129, 40, 13, 252, 46, 211, 88, 150, 129, 150, 87, 117, 73, 167, 159, 245,
    185, 76, 19, 181, 12, 132, 32, 71, 156, 144, 60, 6,
];
const ADD_X2_LE: [u8; 32] = [
    237, 78, 1, 106, 124, 158, 1, 136, 58, 150, 146, 44, 255, 32, 127, 24, 26, 187, 192, 43, 156,
    12, 240, 69, 97, 189, 132, 138, 245, 183, 194, 7,
];
const ADD_Y2_LE: [u8; 32] = [
    215, 23, 250, 120, 132, 120, 214, 43, 116, 92, 72, 164, 6, 23, 54, 223, 23, 154, 76, 247, 163,
    13, 215, 242, 64, 233, 71, 193, 32, 78, 97, 6,
];
const ADD_EXPECTED_X_LE: [u8; 32] = [
    3, 151, 131, 2, 226, 174, 238, 161, 95, 182, 230, 76, 10, 131, 94, 216, 77, 254, 163, 12, 172,
    69, 60, 61, 156, 75, 253, 94, 92, 82, 67, 34,
];
const ADD_EXPECTED_Y_LE: [u8; 32] = [
    21, 201, 149, 241, 72, 125, 94, 174, 185, 125, 83, 50, 117, 237, 14, 24, 35, 71, 150, 53, 204,
    33, 223, 9, 229, 168, 109, 190, 51, 29, 29, 48,
];

const MUL_X_LE: [u8; 32] = [
    183, 159, 10, 26, 139, 38, 2, 29, 230, 72, 86, 174, 215, 3, 71, 76, 213, 185, 229, 156, 180,
    167, 92, 79, 146, 66, 177, 243, 208, 230, 211, 43,
];
const MUL_Y_LE: [u8; 32] = [
    4, 178, 253, 206, 102, 174, 12, 57, 200, 25, 70, 74, 173, 223, 73, 46, 206, 9, 9, 48, 112, 29,
    47, 94, 145, 133, 175, 166, 224, 28, 97, 33,
];
const MUL_SCALAR_LE: [u8; 32] = [
    194, 21, 250, 80, 231, 140, 19, 17, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0,
];
const MUL_EXPECTED_X_LE: [u8; 32] = [
    92, 30, 195, 19, 156, 108, 42, 238, 164, 245, 83, 160, 116, 178, 71, 138, 239, 250, 232, 52,
    212, 41, 190, 228, 202, 83, 33, 152, 106, 141, 10, 7,
];
const MUL_EXPECTED_Y_LE: [u8; 32] = [
    252, 26, 152, 198, 206, 195, 14, 105, 21, 243, 240, 244, 75, 7, 67, 25, 240, 176, 213, 205,
    249, 137, 185, 255, 169, 163, 235, 20, 233, 140, 27, 3,
];

const PAIRING_AX_1_LE: [u8; 32] = [
    89, 63, 196, 36, 96, 193, 49, 221, 100, 166, 173, 118, 170, 167, 255, 129, 51, 25, 161, 187,
    126, 213, 65, 69, 185, 75, 239, 77, 111, 71, 118, 28,
];
const PAIRING_AY_1_LE: [u8; 32] = [
    65, 239, 106, 167, 3, 155, 92, 228, 148, 210, 233, 211, 85, 155, 129, 252, 69, 135, 103, 28,
    129, 226, 254, 4, 226, 115, 246, 32, 41, 221, 52, 48,
];
const PAIRING_BAX_1_LE: [u8; 32] = [
    247, 91, 163, 3, 32, 69, 74, 107, 57, 20, 53, 198, 54, 150, 50, 167, 153, 207, 147, 26, 229,
    136, 216, 75, 108, 212, 245, 191, 94, 209, 157, 32,
];
const PAIRING_BAY_1_LE: [u8; 32] = [
    120, 22, 164, 21, 99, 75, 175, 73, 64, 192, 138, 76, 17, 96, 89, 144, 40, 141, 132, 97, 53,
    180, 52, 139, 250, 59, 72, 1, 202, 17, 191, 4,
];
const PAIRING_BBX_1_LE: [u8; 32] = [
    77, 52, 190, 81, 42, 60, 147, 191, 173, 31, 196, 122, 205, 26, 167, 162, 12, 253, 92, 68, 26,
    173, 162, 55, 53, 201, 207, 246, 74, 50, 184, 43,
];
const PAIRING_BBY_1_LE: [u8; 32] = [
    80, 117, 135, 222, 228, 229, 95, 22, 72, 176, 155, 12, 31, 230, 204, 162, 126, 224, 57, 254,
    198, 32, 95, 132, 249, 27, 12, 243, 76, 42, 10, 18,
];
const PAIRING_AX_2_LE: [u8; 32] = [
    124, 223, 182, 209, 73, 222, 34, 195, 234, 203, 241, 111, 60, 2, 162, 91, 250, 205, 15, 199,
    74, 28, 212, 16, 119, 9, 241, 28, 159, 18, 30, 17,
];
const PAIRING_AY_2_LE: [u8; 32] = [
    17, 244, 107, 106, 206, 250, 83, 56, 167, 112, 56, 185, 133, 53, 136, 162, 252, 66, 242, 43,
    70, 233, 109, 40, 23, 60, 14, 131, 26, 198, 50, 32,
];
const PAIRING_BAX_2_LE: [u8; 32] = [
    194, 18, 243, 174, 183, 133, 228, 151, 18, 231, 169, 53, 51, 73, 170, 241, 37, 93, 251, 49,
    183, 191, 96, 114, 58, 72, 13, 146, 147, 147, 142, 25,
];
const PAIRING_BAY_2_LE: [u8; 32] = [
    237, 246, 146, 217, 92, 189, 222, 70, 221, 218, 94, 247, 212, 34, 67, 103, 121, 68, 92, 94,
    102, 0, 106, 66, 118, 30, 31, 18, 239, 222, 0, 24,
];
const PAIRING_BBX_2_LE: [u8; 32] = [
    91, 151, 34, 209, 220, 218, 172, 85, 243, 142, 179, 112, 51, 49, 75, 188, 149, 51, 12, 105,
    173, 153, 158, 236, 117, 240, 95, 88, 208, 137, 6, 9,
];
const PAIRING_BBY_2_LE: [u8; 32] = [
    170, 125, 250, 102, 1, 204, 230, 76, 123, 211, 67, 12, 105, 231, 209, 227, 143, 64, 203, 141,
    128, 113, 171, 74, 235, 109, 140, 219, 165, 94, 200, 18,
];
const ALL_ONES: [u8; 32] = [0x11; 32];

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
    let actual = altbn128::alt_bn128_add(
        &G1::from(ADD_X1_LE),
        &G1::from(ADD_Y1_LE),
        &G1::from(ADD_X2_LE),
        &G1::from(ADD_Y2_LE),
    );
    let expected = Ok((Fq::from(ADD_EXPECTED_X_LE), Fq::from(ADD_EXPECTED_Y_LE)));
    if actual != expected {
        runtime::print(&format!("left {:?} right {:?}", actual, expected));
        runtime::revert(ApiError::User(line!() as _));
    }
}

fn test_zero_add() {
    if altbn128::alt_bn128_add(&G1::zero(), &G1::zero(), &G1::zero(), &G1::zero())
        != Ok((Fq::zero(), Fq::zero()))
    {
        runtime::revert(ApiError::User(line!() as _));
    }
}

fn test_add_error() {
    let all_ones = G1::from(ALL_ONES);

    if altbn128::alt_bn128_add(&all_ones, &all_ones, &all_ones, &all_ones)
        != Err(Error::InvalidPoint)
    {
        runtime::revert(ApiError::User(line!() as _));
    }
}

fn test_alt_bn128_mul() {
    let x = G1::from(MUL_X_LE);
    let y = G1::from(MUL_Y_LE);
    let scalar = Fr::from(MUL_SCALAR_LE);

    assert_eq!(
        altbn128::alt_bn128_mul(&x, &y, &scalar),
        Ok((Fq::from(MUL_EXPECTED_X_LE), Fq::from(MUL_EXPECTED_Y_LE)))
    );
}
const BIG_SCALAR_0X02: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x02,
];
const BIG_SCALAR_0X0F: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x0f,
];

fn test_zero_multiplication() {
    let x = G1::zero();
    let y = G1::zero();
    let scalar = Fr::from(BIG_SCALAR_0X02);
    assert_eq!(
        altbn128::alt_bn128_mul(&x, &y, &scalar),
        Ok((Fq::zero(), Fq::zero()))
    );
}

fn test_not_on_curve_multiplication() {
    let x = G1::from(ALL_ONES);
    let y = G1::from(ALL_ONES);
    let scalar = Fr::from(BIG_SCALAR_0X0F);
    assert_eq!(
        altbn128::alt_bn128_mul(&x, &y, &scalar),
        Err(Error::InvalidPoint)
    );
}

fn test_alt_bn128_pairing() {
    let all_pairs = [
        Pair {
            ax: Fq::from(PAIRING_AX_1_LE),
            ay: Fq::from(PAIRING_AY_1_LE),
            bax: Fq::from(PAIRING_BAX_1_LE),
            bay: Fq::from(PAIRING_BAY_1_LE),
            bbx: Fq::from(PAIRING_BBX_1_LE),
            bby: Fq::from(PAIRING_BBY_1_LE),
        },
        Pair {
            ax: Fq::from(PAIRING_AX_2_LE),
            ay: Fq::from(PAIRING_AY_2_LE),
            bax: Fq::from(PAIRING_BAX_2_LE),
            bay: Fq::from(PAIRING_BAY_2_LE),
            bbx: Fq::from(PAIRING_BBX_2_LE),
            bby: Fq::from(PAIRING_BBY_2_LE),
        },
    ];

    assert_eq!(altbn128::alt_bn128_pairing(&all_pairs), Ok(true));
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

fn test_alt_bn128_invalid_pairing_args() {
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

pub(crate) fn perform_tests() {
    test_alt_bn128_add();
    test_alt_bn128_mul();
    test_alt_bn128_pairing();

    test_alt_bn128_invalid_pairing_args();
    test_zero_add();
    test_add_error();
    test_zero_multiplication();
    test_not_on_curve_multiplication();
    test_pairing_no_input();
    test_pairing_invalid_a();
    test_pairing_on_curve();
}
