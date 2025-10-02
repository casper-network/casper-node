use core::marker::PhantomData;

use casper_contract_macros::TypeUid;
use casper_executor_wasm_common::{
    error::{CALLEE_GAS_DEPLETED, CALLEE_NOT_CALLABLE, CALLEE_REVERTED, CALLEE_TRAPPED},
    keyspace::Keyspace,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::abi::{AbiDeclaration, CasperABI, Definition, EnumVariant};

use crate::{
    casper,
    compat::types::{CLType, CLTyped},
    prelude::fmt,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

pub use ::bytes::Bytes;
pub type Address = [u8; 32];
pub use bnum::types::{U256, U512};

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
#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(crate = "crate::serializers::borsh", use_discriminant = true)]
pub enum EntityAddr {
    /// PublicKey bytes.
    Account(Address) = 1,
    /// Purse address bytes.
    SmartContract(Address) = 2,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(crate = "crate::serializers::borsh", use_discriminant = true)]
pub enum DelegatorKind {
    /// PublicKey bytes.
    PublicKey(PublicKey) = 0,
    /// Purse address bytes.
    Purse(Address) = 1,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct Reservation {
    /// Delegator kind.
    delegator_kind: DelegatorKind,
    /// Validator public key.
    validator_public_key: PublicKey,
    /// Individual delegation rate.
    delegation_rate: u8,
}

impl Reservation {
    /// Ctor.
    pub fn new(
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        delegation_rate: u8,
    ) -> Self {
        Reservation {
            delegator_kind,
            validator_public_key,
            delegation_rate,
        }
    }
}

#[repr(u32)]
pub enum SystemContractOption {
    Transfer = 0,
    TransferPurse = 1,
    Burn = 2,
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

impl TryFrom<u32> for SystemContractOption {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SystemContractOption::Transfer),
            1 => Ok(SystemContractOption::TransferPurse),
            2 => Ok(SystemContractOption::Burn),
            100 => Ok(SystemContractOption::ActivateBid),
            101 => Ok(SystemContractOption::Bid),
            102 => Ok(SystemContractOption::Withdraw),
            103 => Ok(SystemContractOption::Delegate),
            104 => Ok(SystemContractOption::Undelegate),
            105 => Ok(SystemContractOption::Redelegate),
            106 => Ok(SystemContractOption::AddReservation),
            107 => Ok(SystemContractOption::CancelReservation),
            108 => Ok(SystemContractOption::ChangePublicKey),
            _ => Err(()),
        }
    }
}

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

    pub fn write(&self, value: T) {
        let bytes = borsh::to_vec(&value).unwrap();
        casper::write(Keyspace::NamedKey(self.name), &bytes).unwrap();
    }

    pub fn read(&self) -> Option<T> {
        let bytes = casper::read_into_vec(Keyspace::NamedKey(self.name)).ok()??;
        Some(borsh::from_slice(&bytes).unwrap())
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, TypeUid)]
#[type_uid(crate = "crate::common::type_uid")]
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

impl CLTyped for CallError {
    fn cl_type() -> CLType {
        CLType::U32
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl CasperABI for CallError {
    fn declaration() -> AbiDeclaration {
        "CallError".into()
    }

    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "CalleeReverted".into(),
                    discriminant: 0,
                    decl: None,
                },
                EnumVariant {
                    name: "CalleeTrapped".into(),
                    discriminant: 1,
                    decl: None,
                },
                EnumVariant {
                    name: "CalleeGasDepleted".into(),
                    discriminant: 2,
                    decl: None,
                },
                EnumVariant {
                    name: "CodeNotFound".into(),
                    discriminant: 3,
                    decl: None,
                },
            ],
        }
    }
}

#[repr(u32)]
pub enum CryptoFunctionOption {
    AltBn128Add = 200,
    AltBn128Multiply = 201,
    AltBn128Pairing = 202,
}

impl From<CryptoFunctionOption> for u32 {
    fn from(value: CryptoFunctionOption) -> Self {
        value as u32
    }
}

impl TryFrom<u32> for CryptoFunctionOption {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 200 {
            Ok(CryptoFunctionOption::AltBn128Add)
        } else if value == 201 {
            Ok(CryptoFunctionOption::AltBn128Multiply)
        } else if value == 202 {
            Ok(CryptoFunctionOption::AltBn128Pairing)
        } else {
            Err(())
        }
    }
}
