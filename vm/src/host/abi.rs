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
pub(crate) struct EntryPoint {
    pub(crate) selector: u32,
    pub(crate) fptr: u32, // extern "C" fn(A1) -> (),
    pub(crate) flags: u32,
}

unsafe impl TriviallyTransmutable for EntryPoint {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Manifest {
    pub(crate) entry_points_ptr: u32,
    pub(crate) entry_points_size: u32,
    pub(crate) fallback_fptr: u32,
}

unsafe impl TriviallyTransmutable for Manifest {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]

pub(crate) struct CreateResult {
    pub(crate) package_address: [u8; 32],
    pub(crate) contract_address: [u8; 32],
    pub(crate) version: u32,
}

unsafe impl TriviallyTransmutable for CreateResult {}
