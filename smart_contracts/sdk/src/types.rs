pub type Address = [u8; 32];

#[derive(Debug)]
pub struct Entry {
    pub tag: u64,
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub enum ResultCode {
    /// Called contract returned successfully.
    Success = 0,
    /// Callee contract reverted.
    CalleeReverted = 1,
    /// Called contract trapped.
    CalleeTrapped = 2,
    /// Called contract reached gas limit.
    CalleeGasDepleted = 3,
    Unknown,
}

impl From<u32> for ResultCode {
    fn from(value: u32) -> Self {
        match value {
            0 => ResultCode::Success,
            1 => ResultCode::CalleeReverted,
            2 => ResultCode::CalleeTrapped,
            3 => ResultCode::CalleeGasDepleted,
            _ => ResultCode::Unknown,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CallError {
    CalleeReverted,
    CalleeTrapped,
    CalleeGasDepleted,
    Unknown,
}
