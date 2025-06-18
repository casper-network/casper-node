use safe_transmute::TriviallyTransmutable;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ReadInfo {
    /// Allocated pointer.
    pub(crate) data: u32,
    /// Size in bytes.
    pub(crate) data_size: u32,
    /// Type UID of the data.
    ///
    /// This is a 64-bit unsigned integer that represents the type of the data.
    pub(crate) data_type: u64,
}

unsafe impl TriviallyTransmutable for ReadInfo {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]

pub(crate) struct CreateResult {
    pub(crate) package_address: [u8; 32],
}

unsafe impl TriviallyTransmutable for CreateResult {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct CallResult {
    /// Result of the call.
    pub(crate) call_outcome: u32,
    /// Allocated pointer.
    pub(crate) data_ptr: u32,
    /// Size in bytes.
    pub(crate) data_size: u32,
    /// Type UID of the data.
    ///
    /// This is a 64-bit unsigned integer that represents the type of the data.
    pub(crate) data_type: u64,
}

unsafe impl TriviallyTransmutable for CallResult {}
