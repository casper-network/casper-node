use num_derive::{FromPrimitive, ToPrimitive};
use safe_transmute::TriviallyTransmutable;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ReadInfo {
    /// Allocated pointer.
    pub(crate) data: u32,
    /// Size in bytes.
    pub(crate) data_size: u32,
    /// Value tag.
    pub(crate) tag: u64,
}

unsafe impl TriviallyTransmutable for ReadInfo {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct EntryPoint {
    pub(crate) name_ptr: u32,
    pub(crate) name_len: u32,

    pub(crate) params_ptr: u32, // pointer of pointers (preferred 'static lifetime)
    pub(crate) params_size: u32,

    pub(crate) fptr: u32, // extern "C" fn(A1) -> (),

    pub(crate) flags: u32,
}

unsafe impl TriviallyTransmutable for EntryPoint {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Manifest {
    pub(crate) entry_points_ptr: u32,
    pub(crate) entry_points_size: u32,
}

unsafe impl TriviallyTransmutable for Manifest {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Param {
    pub(crate) name_ptr: u32,
    pub(crate) name_len: u32,
    pub(crate) ty: u32,
}

unsafe impl TriviallyTransmutable for Param {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]

pub(crate) struct CreateResult {
    pub(crate) package_address: [u8; 32],
    pub(crate) contract_address: [u8; 32],
    pub(crate) version: u32,
}

unsafe impl TriviallyTransmutable for CreateResult {}
