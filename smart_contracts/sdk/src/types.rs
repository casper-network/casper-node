use casper_executor_wasm_common::error::{
    CALLEE_GAS_DEPLETED, CALLEE_NOT_CALLABLE, CALLEE_REVERTED, CALLEE_TRAPPED,
};

use crate::{
    abi::{CasperABI, Declaration, Definition, EnumVariant},
    prelude::fmt,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

pub type Address = [u8; 32];
pub use bnum::types::U256;

// pub const ED25519_TAG: u8 = 1;
// pub const SECP256K1_TAG: u8 = 2;

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

#[repr(u32)]
pub enum SystemContractOption {
    Transfer = 0,
    Burn = 1,
    ActivateBid = 100,
    Bid = 101,
    Withdraw = 102,
    Delegate = 103,
    Undelegate = 104,
    Redelegate = 105,
    AddReservation = 106,
    CancelReservation = 107,
    ChangePublicKey = 108,
}

impl From<SystemContractOption> for u32 {
    fn from(value: SystemContractOption) -> Self {
        value as u32
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
