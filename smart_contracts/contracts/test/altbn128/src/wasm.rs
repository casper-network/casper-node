//! Consts for wasm-version of a bn curves.
//!
//! Counter-intuitively, these values are encoded in big-endian form as the bn crate expects u256
//! integers to be encoded in big-endian form.
//!
//! The host version of the bn crate expects u256 integers to be encoded in little-endian form as
//! it's more natural and expectedly more efficient for of transforming u256 values to and from
//! host.
#[allow(unused_imports)]
use alloc::vec::Vec;
#[allow(unused_imports)]
use bn::{AffineG1, AffineG2, Fq, Fr, Group, G1};
#[allow(unused_imports)]
use casper_contract::contract_api::builtins::altbn128::Error;

#[allow(dead_code)]
mod consts {
    pub const ADD_X1_BE: [u8; 32] = [
        24, 177, 138, 207, 180, 194, 195, 2, 118, 219, 84, 17, 54, 142, 113, 133, 179, 17, 221, 18,
        70, 145, 97, 12, 93, 59, 116, 3, 78, 9, 61, 201,
    ];
    pub const ADD_Y1_BE: [u8; 32] = [
        6, 60, 144, 156, 71, 32, 132, 12, 181, 19, 76, 185, 245, 159, 167, 73, 117, 87, 150, 129,
        150, 88, 211, 46, 252, 13, 40, 129, 152, 243, 114, 102,
    ];
    pub const ADD_X2_BE: [u8; 32] = [
        7, 194, 183, 245, 138, 132, 189, 97, 69, 240, 12, 156, 43, 192, 187, 26, 24, 127, 32, 255,
        44, 146, 150, 58, 136, 1, 158, 124, 106, 1, 78, 237,
    ];

    pub const ADD_Y2_BE: [u8; 32] = [
        6, 97, 78, 32, 193, 71, 233, 64, 242, 215, 13, 163, 247, 76, 154, 23, 223, 54, 23, 6, 164,
        72, 92, 116, 43, 214, 120, 132, 120, 250, 23, 215,
    ];

    pub const ADD_EXPECTED_X_BE: [u8; 32] = [
        34, 67, 82, 92, 94, 253, 75, 156, 61, 60, 69, 172, 12, 163, 254, 77, 216, 94, 131, 10, 76,
        230, 182, 95, 161, 238, 174, 226, 2, 131, 151, 3,
    ];
    pub const ADD_EXPECTED_Y_BE: [u8; 32] = [
        48, 29, 29, 51, 190, 109, 168, 229, 9, 223, 33, 204, 53, 150, 71, 35, 24, 14, 237, 117, 50,
        83, 125, 185, 174, 94, 125, 72, 241, 149, 201, 21,
    ];

    pub const MUL_X_BE: [u8; 32] = [
        43, 211, 230, 208, 243, 177, 66, 146, 79, 92, 167, 180, 156, 229, 185, 213, 76, 71, 3, 215,
        174, 86, 72, 230, 29, 2, 38, 139, 26, 10, 159, 183,
    ];
    pub const MUL_Y_BE: [u8; 32] = [
        33, 97, 28, 224, 166, 175, 133, 145, 94, 47, 29, 112, 48, 9, 9, 206, 46, 73, 223, 173, 74,
        70, 25, 200, 57, 12, 174, 102, 206, 253, 178, 4,
    ];
    pub const MUL_SCALAR_BE: [u8; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 17, 19, 140, 231,
        80, 250, 21, 194,
    ];
    pub const MUL_EXPECTED_X_BE: [u8; 32] = [
        7, 10, 141, 106, 152, 33, 83, 202, 228, 190, 41, 212, 52, 232, 250, 239, 138, 71, 178, 116,
        160, 83, 245, 164, 238, 42, 108, 156, 19, 195, 30, 92,
    ];
    pub const MUL_EXPECTED_Y_BE: [u8; 32] = [
        3, 27, 140, 233, 20, 235, 163, 169, 255, 185, 137, 249, 205, 213, 176, 240, 25, 67, 7, 75,
        244, 240, 243, 21, 105, 14, 195, 206, 198, 152, 26, 252,
    ];

    pub const PAIRING_AX_1_BE: [u8; 32] = [
        28, 118, 71, 111, 77, 239, 75, 185, 69, 65, 213, 126, 187, 161, 25, 51, 129, 255, 167, 170,
        118, 173, 166, 100, 221, 49, 193, 96, 36, 196, 63, 89,
    ];
    pub const PAIRING_AY_1_BE: [u8; 32] = [
        48, 52, 221, 41, 32, 246, 115, 226, 4, 254, 226, 129, 28, 103, 135, 69, 252, 129, 155, 85,
        211, 233, 210, 148, 228, 92, 155, 3, 167, 106, 239, 65,
    ];
    pub const PAIRING_BAX_1_BE: [u8; 32] = [
        32, 157, 209, 94, 191, 245, 212, 108, 75, 216, 136, 229, 26, 147, 207, 153, 167, 50, 150,
        54, 198, 53, 20, 57, 107, 74, 69, 32, 3, 163, 91, 247,
    ];
    pub const PAIRING_BAY_1_BE: [u8; 32] = [
        4, 191, 17, 202, 1, 72, 59, 250, 139, 52, 180, 53, 97, 132, 141, 40, 144, 89, 96, 17, 76,
        138, 192, 64, 73, 175, 75, 99, 21, 164, 22, 120,
    ];
    pub const PAIRING_BBX_1_BE: [u8; 32] = [
        43, 184, 50, 74, 246, 207, 201, 53, 55, 162, 173, 26, 68, 92, 253, 12, 162, 167, 26, 205,
        122, 196, 31, 173, 191, 147, 60, 42, 81, 190, 52, 77,
    ];
    pub const PAIRING_BBY_1_BE: [u8; 32] = [
        18, 10, 42, 76, 243, 12, 27, 249, 132, 95, 32, 198, 254, 57, 224, 126, 162, 204, 230, 31,
        12, 155, 176, 72, 22, 95, 229, 228, 222, 135, 117, 80,
    ];
    pub const PAIRING_AX_2_BE: [u8; 32] = [
        17, 30, 18, 159, 28, 241, 9, 119, 16, 212, 28, 74, 199, 15, 205, 250, 91, 162, 2, 60, 111,
        241, 203, 234, 195, 34, 222, 73, 209, 182, 223, 124,
    ];
    pub const PAIRING_AY_2_BE: [u8; 32] = [
        32, 50, 198, 26, 131, 14, 60, 23, 40, 109, 233, 70, 43, 242, 66, 252, 162, 136, 53, 133,
        185, 56, 112, 167, 56, 83, 250, 206, 106, 107, 244, 17,
    ];
    pub const PAIRING_BAX_2_BE: [u8; 32] = [
        25, 142, 147, 147, 146, 13, 72, 58, 114, 96, 191, 183, 49, 251, 93, 37, 241, 170, 73, 51,
        53, 169, 231, 18, 151, 228, 133, 183, 174, 243, 18, 194,
    ];
    pub const PAIRING_BAY_2_BE: [u8; 32] = [
        24, 0, 222, 239, 18, 31, 30, 118, 66, 106, 0, 102, 94, 92, 68, 121, 103, 67, 34, 212, 247,
        94, 218, 221, 70, 222, 189, 92, 217, 146, 246, 237,
    ];
    pub const PAIRING_BBX_2_BE: [u8; 32] = [
        9, 6, 137, 208, 88, 95, 240, 117, 236, 158, 153, 173, 105, 12, 51, 149, 188, 75, 49, 51,
        112, 179, 142, 243, 85, 172, 218, 220, 209, 34, 151, 91,
    ];
    pub const PAIRING_BBY_2_BE: [u8; 32] = [
        18, 200, 94, 165, 219, 140, 109, 235, 74, 171, 113, 128, 141, 203, 64, 143, 227, 209, 231,
        105, 12, 67, 211, 123, 76, 230, 204, 1, 102, 250, 125, 170,
    ];
}
use consts::*;

#[cfg(feature = "wasm_add_test")]
fn wasm_add_test() {
    let p1 = bn_point_from_coords(&ADD_X1_BE, &ADD_Y1_BE);
    let p2 = bn_point_from_coords(&ADD_X2_BE, &ADD_Y2_BE);

    let mut x = None;
    let mut y = None;

    if let Some(sum) = AffineG1::from_jacobian(p1 + p2) {
        x = Some(sum.x());
        y = Some(sum.y());
    }

    assert_eq!(
        (x, y),
        (
            Some(Fq::from_slice(&ADD_EXPECTED_X_BE).unwrap()),
            Some(Fq::from_slice(&ADD_EXPECTED_Y_BE).unwrap())
        )
    );
}
#[allow(dead_code)]
fn bn_point_from_coords(x: &[u8; 32], y: &[u8; 32]) -> G1 {
    let px = Fq::from_slice(x).unwrap();
    let py = Fq::from_slice(y).unwrap();

    if px == Fq::zero() && py == Fq::zero() {
        G1::zero()
    } else {
        AffineG1::new(px, py).unwrap().into()
    }
}
#[cfg(feature = "wasm_mul_test")]
fn wasm_mul_test() {
    fn bn_fq_to_be_bytes(bn_fq: Fq) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bn_fq.to_big_endian(&mut bytes).unwrap();
        bytes
    }

    let p = bn_point_from_coords(&MUL_X_BE, &MUL_Y_BE);

    let mut x = None;
    let mut y = None;
    let fr = Fr::from_slice(&MUL_SCALAR_BE).unwrap();

    if let Some(product) = AffineG1::from_jacobian(p * fr) {
        x = Some(bn_fq_to_be_bytes(product.x()));
        y = Some(bn_fq_to_be_bytes(product.y()));
    }

    assert_eq!((x, y), (Some(MUL_EXPECTED_X_BE), Some(MUL_EXPECTED_Y_BE)));
}

pub fn perform_tests() {
    // Wasm-only tests (compile BN crate to Wassm)
    #[cfg(feature = "wasm_add_test")]
    {
        wasm_add_test();
    }

    #[cfg(feature = "wasm_mul_test")]
    {
        wasm_mul_test();
    }

    #[cfg(feature = "wasm_pairing_test")]
    {
        wasm_pairing_test();
    }
}

#[cfg(feature = "wasm_pairing_test")]
pub fn wasm_alt_bn128_pairing(
    values: &[(&[u8], &[u8], &[u8], &[u8], &[u8], &[u8])],
) -> Result<bool, Error> {
    let mut pairs = Vec::with_capacity(values.len());

    for (ax, ay, bay, bax, bby, bbx) in values {
        let ax = Fq::from_slice(&ax).map_err(|_| Error::InvalidAx)?;
        let ay = Fq::from_slice(&ay).map_err(|_| Error::InvalidAy)?;
        let bay = Fq::from_slice(&bay).map_err(|_| Error::InvalidBay)?;
        let bax = Fq::from_slice(&bax).map_err(|_| Error::InvalidBax)?;
        let bby = Fq::from_slice(&bby).map_err(|_| Error::InvalidBby)?;
        let bbx = Fq::from_slice(&bbx).map_err(|_| Error::InvalidBbx)?;

        let g1_a = {
            if ax.is_zero() && ay.is_zero() {
                bn::G1::zero()
            } else {
                bn::AffineG1::new(ax, ay)
                    .map_err(|_| Error::InvalidA)?
                    .into()
            }
        };
        let g1_b = {
            let ba = bn::Fq2::new(bax, bay);
            let bb = bn::Fq2::new(bbx, bby);

            if ba.is_zero() && bb.is_zero() {
                bn::G2::zero()
            } else {
                bn::AffineG2::new(ba, bb)
                    .map_err(|_| Error::InvalidB)?
                    .into()
            }
        };

        pairs.push((g1_a, g1_b));
    }

    Ok(bn::pairing_batch(pairs.as_slice()) == bn::Gt::one())
}

#[cfg(feature = "wasm_pairing_test")]
fn wasm_pairing_test() {
    assert_eq!(
        wasm_alt_bn128_pairing(&[
            (
                &PAIRING_AX_1_BE,
                &PAIRING_AY_1_BE,
                &PAIRING_BAX_1_BE,
                &PAIRING_BAY_1_BE,
                &PAIRING_BBX_1_BE,
                &PAIRING_BBY_1_BE
            ),
            (
                &PAIRING_AX_2_BE,
                &PAIRING_AY_2_BE,
                &PAIRING_BAX_2_BE,
                &PAIRING_BAY_2_BE,
                &PAIRING_BBX_2_BE,
                &PAIRING_BBY_2_BE
            ),
        ]),
        Ok(true)
    );
}
