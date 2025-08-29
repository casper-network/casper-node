use core::marker::PhantomData;

use casper_executor_wasm_common::{
    error::{CALLEE_GAS_DEPLETED, CALLEE_NOT_CALLABLE, CALLEE_REVERTED, CALLEE_TRAPPED},
    keyspace::Keyspace,
};

use crate::{
    abi::{CasperABI, Declaration, Definition, EnumVariant},
    casper,
    prelude::fmt,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

pub use ::bytes::Bytes;
pub type Address = [u8; 32];
pub use bnum::types::U256;

pub struct NamedKey<T: BorshSerialize + BorshDeserialize> {
    name: &'static str,
    _marker: PhantomData<T>,
}

impl<T: BorshSerialize + BorshDeserialize> NamedKey<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _marker: PhantomData,
        }
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Populate ABI definitions for the value type `T` of this named key.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn collect_abi(&self, definitions: &mut crate::abi::Definitions)
    where
        T: CasperABI,
    {
        definitions.populate_one::<T>();
    }

    /// Return the ABI declaration string for the value type `T` of this named key.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn declaration(&self) -> Declaration
    where
        T: CasperABI,
    {
        <T as CasperABI>::declaration()
    }

    pub fn write(&self, value: T) {
        let bytes = borsh::to_vec(&value).unwrap();
        casper::write(Keyspace::NamedKey(self.name), &bytes).unwrap();
    }

    pub fn read(&self) -> Option<T> {
        let bytes = casper::read_into_vec(Keyspace::NamedKey(self.name)).ok()??;
        Some(borsh::from_slice(&bytes).unwrap())
    }
}

/// Bytes for Ed25519 public key.
pub type AddressEd25519 = [u8; 32];

/// Bytes for Secp256k1 public key.
pub type AddressSecp256k1 = [u8; 33];

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(crate = "crate::serializers::borsh", use_discriminant = true)]
pub enum PublicKey {
    /// Ed25519 public key bytes.
    Ed25519(AddressEd25519) = 1,
    /// secp256k1 public key sec1 bytes.
    Secp256k1(AddressSecp256k1) = 2,
}

impl From<AddressEd25519> for PublicKey {
    fn from(addr: AddressEd25519) -> Self {
        PublicKey::Ed25519(addr)
    }
}

impl From<AddressSecp256k1> for PublicKey {
    fn from(addr: AddressSecp256k1) -> Self {
        PublicKey::Secp256k1(addr)
    }
}

/// A type of hashing algorithm.
#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(crate = "crate::serializers::borsh", use_discriminant = true)]
pub enum HashAlgorithm {
    /// Blake2b
    Blake2b = 0,
    /// Blake3
    Blake3 = 1,
    /// Sha256,
    Sha256 = 2,
    /// Keccak256
    Keccak256 = 3,
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
