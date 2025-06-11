use safe_transmute::TriviallyTransmutable;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ReadInfo {
    /// Allocated pointer.
    pub(crate) data: u32,
    /// Size in bytes.
    pub(crate) data_size: u32,
}

unsafe impl TriviallyTransmutable for ReadInfo {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]

pub(crate) struct CreateResult {
    pub(crate) package_address: [u8; 32],
}

unsafe impl TriviallyTransmutable for CreateResult {}
