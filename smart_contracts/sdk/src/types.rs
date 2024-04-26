use borsh::{BorshDeserialize, BorshSerialize};

use crate::abi::{CasperABI, Definition, EnumVariant};

pub type Address = [u8; 32];

#[derive(Debug)]
pub struct Entry(pub(crate) ());

#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum CallError {
    CalleeReverted,
    CalleeTrapped,
    CalleeGasDepleted,
    CodeNotFound,
}

impl TryFrom<u32> for CallError {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(CallError::CalleeReverted),
            2 => Ok(CallError::CalleeTrapped),
            3 => Ok(CallError::CalleeGasDepleted),
            4 => Ok(CallError::CodeNotFound),
            _ => Err(()),
        }
    }
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
                EnumVariant {
                    name: "CodeNotFound".into(),
                    discriminant: 3,
                    decl: <()>::declaration(),
                },
            ],
        }
    }
}
