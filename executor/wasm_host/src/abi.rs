use borsh::BorshSerialize;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, BorshSerialize)]

pub(crate) struct CreateResult {
    pub(crate) package_addr: [u8; 32],
}

const _: () = assert!(
    std::mem::size_of::<CreateResult>() == 32,
    "CreateResult must be 32 bytes"
);

#[derive(Clone, Copy, BorshSerialize, Debug, PartialEq)]
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
