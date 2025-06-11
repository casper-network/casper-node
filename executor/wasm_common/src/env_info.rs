use safe_transmute::TriviallyTransmutable;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct EnvInfo {
    pub block_time: u64,
    pub transferred_value: u64,
    pub caller_addr: [u8; 32],
    pub caller_kind: u32,
    pub callee_addr: [u8; 32],
    pub callee_kind: u32,
}

unsafe impl TriviallyTransmutable for EnvInfo {}
