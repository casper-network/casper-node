use borsh::BorshDeserialize;

#[derive(Debug, BorshDeserialize)]
#[repr(C)]
pub struct EnvInfo {
    pub protocol_version_major: u32,
    pub protocol_version_minor: u32,
    pub protocol_version_patch: u32,
    pub block_height: u64,
    pub block_time: u64,
    pub parent_block_hash: [u8; 32],
    pub transferred_value: u64,
    pub caller_addr: [u8; 32],
    pub caller_kind: u32,
    pub callee_addr: [u8; 32],
    pub callee_kind: u32,
}

/// Signature of a function pointer that a host understands.
pub type Fptr = extern "C" fn() -> ();

#[derive(Debug)]
#[repr(C)]
pub struct ReadInfo {
    pub data_ptr: *const u8,
    /// Size in bytes
    pub data_size: usize,
}

#[repr(C)]
#[derive(Debug, BorshDeserialize)]
pub struct CreateResult {
    pub contract_address: [u8; 32],
}

#[repr(C)]
#[derive(Debug)]
pub struct UpgradeResult {
    pub package_address: [u8; 32],
    pub contract_address: [u8; 32],
    pub version: u32,
}

extern "C" {
    pub fn casper_ffi(
        ffi_opt: u32,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8, /* For capturing output
                                                                         * data */
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32;
}
