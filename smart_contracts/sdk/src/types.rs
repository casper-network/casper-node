use core::marker::PhantomData;

use casper_executor_wasm_common::{error::{
    CALLEE_GAS_DEPLETED, CALLEE_NOT_CALLABLE, CALLEE_REVERTED, CALLEE_TRAPPED,
}, keyspace::Keyspace};

use crate::{
    abi::{CasperABI, Declaration, Definition, EnumVariant}, casper, prelude::fmt, serializers::borsh::{BorshDeserialize, BorshSerialize}
};

pub type Address = [u8; 32];
pub use bnum::types::U256;

#[repr(C)]
pub struct StableKey<T: BorshSerialize + BorshDeserialize> {
    name: &'static str,
    _marker: PhantomData<T>,
}

impl<T: BorshSerialize + BorshDeserialize> StableKey<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _marker: PhantomData,
        }
    }

    pub fn write(&self, value: T) {
        let bytes = borsh::to_vec(&value).unwrap();
        casper::write(Keyspace::NamedKey(self.name), &bytes).unwrap();
    }

    pub fn read(&self) -> Option<T> {
        let bytes = casper::read_into_vec(Keyspace::NamedKey(self.name)).ok()??;
        borsh::from_slice(&bytes).unwrap()
    }
}

// Keep in sync with [`casper_executor_wasm_common::error::CallError`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(crate = "crate::serializers::borsh")]
pub enum CallError {
    CalleeReverted,
    CalleeTrapped,
    CalleeGasDepleted,
    NotCallable,
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::CalleeReverted => write!(f, "callee reverted"),
            CallError::CalleeTrapped => write!(f, "callee trapped"),
            CallError::CalleeGasDepleted => write!(f, "callee gas depleted"),
            CallError::NotCallable => write!(f, "not callable"),
        }
    }
}

impl TryFrom<u32> for CallError {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            CALLEE_REVERTED => Ok(Self::CalleeReverted),
            CALLEE_TRAPPED => Ok(Self::CalleeTrapped),
            CALLEE_GAS_DEPLETED => Ok(Self::CalleeGasDepleted),
            CALLEE_NOT_CALLABLE => Ok(Self::NotCallable),
            _ => Err(()),
        }
    }
}

impl CasperABI for CallError {
    fn populate_definitions(_definitions: &mut crate::abi::Definitions) {}

    fn declaration() -> Declaration {
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
