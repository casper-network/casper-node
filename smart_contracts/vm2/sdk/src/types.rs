use core::marker::PhantomData;

use casper_executor_wasm_common::{
    error::{
        CALLEE_API_ERROR, CALLEE_GAS_DEPLETED, CALLEE_INPUT_INVALID, CALLEE_NOT_CALLABLE,
        CALLEE_ROLLED_BACK, CALLEE_TRAPPED,
    },
    keyspace::Keyspace,
};

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
use crate::abi::{CasperABI, Declaration, Definition, EnumVariant};

use crate::{
    casper,
    prelude::fmt,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

pub use ::bytes::Bytes;
pub type Address = [u8; 32];
pub use bnum::types::U256;

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

    /// Populate ABI definitions for the value type `T` of this named key.
    #[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
    pub fn collect_abi(&self, definitions: &mut crate::abi::Definitions)
    where
        T: CasperABI,
    {
        definitions.populate_one::<T>();
    }

    /// Return the ABI declaration string for the value type `T` of this named key.
    #[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
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
#[borsh(crate = "crate::serializers::borsh", use_discriminant = true)]
pub enum CallError {
    CalleeRolledBack = 1,
    CalleeTrapped = 2,
    InputInvalid = 3,
    CalleeGasDepleted = 4,
    NotCallable = 5,
    Api = 6,
    NoActiveContract = 7,
    CodeNotFound = 8,
    EntityNotFound = 9,
    LockedPackage = 10,
    InvalidOutput = 255,
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::CalleeRolledBack => write!(f, "callee rolled back"),
            CallError::CalleeTrapped => write!(f, "callee trapped"),
            CallError::CalleeGasDepleted => write!(f, "callee gas depleted"),
            CallError::NotCallable => write!(f, "not callable"),
            CallError::InputInvalid => write!(f, "input invalid"),
            CallError::Api => write!(f, "api"),
            CallError::NoActiveContract => write!(f, "no active contract"),
            CallError::CodeNotFound => write!(f, "code not found"),
            CallError::EntityNotFound => write!(f, "entity not found"),
            CallError::LockedPackage => write!(f, "locked package"),
            CallError::InvalidOutput => write!(f, "invalid output"),
        }
    }
}

impl TryFrom<u32> for CallError {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            CALLEE_ROLLED_BACK => Ok(Self::CalleeRolledBack),
            CALLEE_TRAPPED => Ok(Self::CalleeTrapped),
            CALLEE_GAS_DEPLETED => Ok(Self::CalleeGasDepleted),
            CALLEE_NOT_CALLABLE => Ok(Self::NotCallable),
            CALLEE_INPUT_INVALID => Ok(Self::InputInvalid),
            CALLEE_API_ERROR => Ok(Self::Api),
            _ => Err(()),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
impl CasperABI for CallError {
    fn populate_definitions(_definitions: &mut crate::abi::Definitions) {}

    fn declaration() -> Declaration {
        "CallError".into()
    }
    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "CalleeRolledBack".into(),
                    discriminant: 1,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "CalleeTrapped".into(),
                    discriminant: 2,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "InputInvalid".into(),
                    discriminant: 3,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "CalleeGasDepleted".into(),
                    discriminant: 4,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "NotCallable".into(),
                    discriminant: 5,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "Api".into(),
                    discriminant: 6,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "InvalidOutput".into(),
                    discriminant: 7,
                    decl: <()>::declaration(),
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
    GenericHash = 203,
    RecoverSecp256K1 = 204,
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
        } else if value == 203 {
            Ok(CryptoFunctionOption::GenericHash)
        } else if value == 204 {
            Ok(CryptoFunctionOption::RecoverSecp256K1)
        } else {
            Err(())
        }
    }
}

#[repr(u32)]
pub enum EmitFunctionOption {
    PrintStd = 300,
    Native = 301,
}

impl From<EmitFunctionOption> for u32 {
    fn from(value: EmitFunctionOption) -> Self {
        value as u32
    }
}

impl TryFrom<u32> for EmitFunctionOption {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 300 {
            Ok(EmitFunctionOption::PrintStd)
        } else if value == 301 {
            Ok(EmitFunctionOption::Native)
        } else {
            Err(())
        }
    }
}

#[repr(u32)]
pub enum GlobalStateFunctionOption {
    Read = 400,
    Write = 401,
    Remove = 402,
    GetBalance = 403,
    GetInfo = 404,
    Create = 405,
}

impl From<GlobalStateFunctionOption> for u32 {
    fn from(value: GlobalStateFunctionOption) -> Self {
        value as u32
    }
}

impl TryFrom<u32> for GlobalStateFunctionOption {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 400 {
            Ok(GlobalStateFunctionOption::Read)
        } else if value == 401 {
            Ok(GlobalStateFunctionOption::Write)
        } else if value == 402 {
            Ok(GlobalStateFunctionOption::Remove)
        } else if value == 403 {
            Ok(GlobalStateFunctionOption::GetBalance)
        } else if value == 404 {
            Ok(GlobalStateFunctionOption::GetInfo)
        } else if value == 405 {
            Ok(GlobalStateFunctionOption::Create)
        } else {
            Err(())
        }
    }
}

#[repr(u32)]
pub enum ControlFunctionOption {
    Call = 500,
    Upgrade = 501,
}

impl From<ControlFunctionOption> for u32 {
    fn from(value: ControlFunctionOption) -> Self {
        value as u32
    }
}

impl TryFrom<u32> for ControlFunctionOption {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            500 => ControlFunctionOption::Call,
            501 => ControlFunctionOption::Upgrade,
            _ => return Err(()),
        })
    }
}

#[repr(u32)]
pub enum IOFunctionOption {
    Return = 600,
    CopyInput = 601,
}

impl From<IOFunctionOption> for u32 {
    fn from(value: IOFunctionOption) -> Self {
        value as u32
    }
}

impl TryFrom<u32> for IOFunctionOption {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            600 => IOFunctionOption::Return,
            601 => IOFunctionOption::CopyInput,
            _ => return Err(()),
        })
    }
}
