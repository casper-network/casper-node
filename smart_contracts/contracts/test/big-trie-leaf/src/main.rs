#![no_std]
#![no_main]

use core::panic::PanicInfo;

const USER_ERROR_CODE_OFFSET: u32 = 65536;

const KB: usize = 1024;
const MB: usize = 1024 * KB;
const VEC_LEN: usize = 8 * MB;

const KEY_TAG_UREF: u8 = 2;

const CL_TYPE_TAG_U8: u8 = 3;
const CL_TYPE_TAG_UNIT: u8 = 9;
const CL_TYPE_TAG_LIST: u8 = 14;

const CLTYPE_LEN: usize = 2;
const CL_TYPE: [u8; CLTYPE_LEN] = [CL_TYPE_TAG_LIST, CL_TYPE_TAG_U8];

const LENGTH_PREFIX_LEN: usize = 4;
const CL_VALUE_OVERHEAD: usize = LENGTH_PREFIX_LEN + CLTYPE_LEN;

const UREF_SERIALIZED_LENGTH: usize = 32 + 1;

const KEY_NAME: [u8; 8] = [4, 0, 0, 0, b'd', b'a', b't', b'a']; // "data"
const CL_VALUE_UNIT: [u8; 4 + 1] = [0, 0, 0, 0, CL_TYPE_TAG_UNIT];

const SERIALIZED_VEC_LENGTH: usize = LENGTH_PREFIX_LEN + VEC_LEN;
const BUFFER_LEN: usize = SERIALIZED_VEC_LENGTH + CL_VALUE_OVERHEAD;
static mut BUFFER: [u8; BUFFER_LEN] = [0; BUFFER_LEN];

type KeyBytes = [u8; UREF_SERIALIZED_LENGTH + 1];

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    revert_user(42)
}

fn revert_user(user_code: u32) -> ! {
    unsafe { casper_revert(USER_ERROR_CODE_OFFSET + user_code) }
}

extern "C" {
    fn casper_revert(status: u32) -> !;
    fn casper_put_key(name_ptr: *const u8, name_size: usize, key_ptr: *const u8, key_size: usize);
    fn casper_write(key_ptr: *const u8, key_size: usize, value_ptr: *const u8, value_size: usize);
    fn casper_new_uref(uref_ptr: *mut u8, value_ptr: *const u8, value_size: usize);
    fn casper_get_blocktime(dest_ptr: *const u8);
}

fn get_blocktime() -> u64 {
    let u64_bytes = [0u8; 8];
    unsafe { casper_get_blocktime(u64_bytes.as_ptr()) }
    u64::from_le_bytes(u64_bytes)
}

fn new_raw_uref_key() -> KeyBytes {
    let mut key_bytes = [0; UREF_SERIALIZED_LENGTH + 1];
    key_bytes[0] = KEY_TAG_UREF;
    let uref_part = &mut key_bytes[1..];
    unsafe {
        casper_new_uref(
            uref_part.as_mut_ptr(),
            CL_VALUE_UNIT.as_ptr(),
            CL_VALUE_UNIT.len(),
        )
    };
    key_bytes
}

fn put_raw_key_bytes(name_bytes: &[u8], key_bytes: &[u8]) {
    unsafe {
        casper_put_key(
            name_bytes.as_ptr(),
            name_bytes.len(),
            key_bytes.as_ptr(),
            key_bytes.len(),
        )
    }
}

fn write_raw_cl_value(key_bytes: &KeyBytes, raw_cl_value_bytes: &[u8]) {
    unsafe {
        casper_write(
            key_bytes.as_ptr(),
            key_bytes.len(),
            raw_cl_value_bytes.as_ptr(),
            raw_cl_value_bytes.len(),
        )
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let raw_uref_key = new_raw_uref_key();
    let cl_length_prefix = SERIALIZED_VEC_LENGTH as u32;
    let vec_length_prefix = VEC_LEN as u32;
    let buffer = unsafe { &mut BUFFER };
    buffer[0..LENGTH_PREFIX_LEN].copy_from_slice(&cl_length_prefix.to_le_bytes());

    {
        // vector contents

        let mut pos = LENGTH_PREFIX_LEN;

        buffer[pos..pos + 4].copy_from_slice(&vec_length_prefix.to_le_bytes());
        pos += 4;

        let block_time = get_blocktime();
        buffer[pos..pos + 8].copy_from_slice(&block_time.to_be_bytes());
    }

    buffer[BUFFER_LEN - CL_TYPE.len()..].copy_from_slice(&CL_TYPE);
    write_raw_cl_value(&raw_uref_key, &buffer[..]);
    put_raw_key_bytes(&KEY_NAME, &raw_uref_key);
}
