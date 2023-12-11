use serde::{Deserialize, Serialize};

const BOOL_TYPE_ID: u32 = 0;
const STRING_TYPE_ID: u32 = 1;
const UNIT_TYPE_ID: u32 = 2;
const ANY_TYPE_ID: u32 = 3;
const U32_TYPE_ID: u32 = 4;

pub trait CLTyped {
    const TYPE_ID: u32;
    fn cl_type() -> CLType;
}

impl CLTyped for String {
    const TYPE_ID: u32 = STRING_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::String
    }
}
impl CLTyped for bool {
    const TYPE_ID: u32 = BOOL_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::Bool
    }
}

impl CLTyped for () {
    const TYPE_ID: u32 = UNIT_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::Unit
    }
}

impl CLTyped for u32 {
    const TYPE_ID: u32 = U32_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::U32
    }
}

#[repr(u32)]
#[derive(Debug, Serialize, Deserialize)]
pub enum CLType {
    Bool,
    String,
    Unit,
    Any,
    U32,
}
