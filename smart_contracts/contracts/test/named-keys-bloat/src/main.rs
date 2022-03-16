#![no_main]

const BUFFER_LEN: usize = 65536;
static mut BUFFER: [u8; BUFFER_LEN] = [97u8; BUFFER_LEN];

extern "C" {
    fn casper_put_key(name_ptr: *const u8, name_size: usize, key_ptr: *const u8, key_size: usize);
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

const TRIE_LIMIT: usize = 8 * 1024 * 1024;

#[no_mangle]
pub extern "C" fn call() {
    let mut raw_hash_bytes: [u8; 33] = [0; 33];
    raw_hash_bytes[0] = 1; // hash

    // String's length prefix
    let buffer = unsafe { &mut BUFFER };

    {
        let length = BUFFER_LEN - 4;
        let length_bytes = length.to_le_bytes();
        buffer[0..4].copy_from_slice(&length_bytes);
    }

    let buffer = unsafe { &mut BUFFER };

    let exceed_storage_limit = TRIE_LIMIT / (BUFFER_LEN + raw_hash_bytes.len()) + 1;

    for i in 0..exceed_storage_limit {
        let is = format!("{:0width$}", i, width = 10);
        buffer[BUFFER_LEN - is.len()..].copy_from_slice(is.as_bytes());
        put_raw_key_bytes(buffer, &raw_hash_bytes);
    }
}
