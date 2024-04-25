use borsh::{BorshDeserialize, BorshSerialize};

use crate::abi::{CasperABI, Definition, EnumVariant};

pub type Address = [u8; 32];

#[derive(Debug)]
pub struct Entry(pub(crate) ());

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
}

impl From<u32> for ResultCode {
    fn from(value: u32) -> Self {
        match value {
            0 => ResultCode::Success,
            1 => ResultCode::CalleeReverted,
            2 => ResultCode::CalleeTrapped,
            3 => ResultCode::CalleeGasDepleted,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum CallError {
    CalleeReverted,
    CalleeTrapped,
    CalleeGasDepleted,
}

impl CasperABI for CallError {
    fn populate_definitions(definitions: &mut crate::abi::Definitions) {}

    fn declaration() -> crate::abi::Declaration {
        "CallError".into()
    }

    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "CalleeReverted".into(),
                    discriminant: 0,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "CalleeTrapped".into(),
                    discriminant: 1,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "CalleeGasDepleted".into(),
                    discriminant: 2,
                    decl: <()>::declaration(),
                },
            ],
        }
    }
}
