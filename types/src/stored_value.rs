mod type_mismatch;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::{convert::TryFrom, fmt::Debug};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};
use serde_bytes::ByteBuf;

use crate::{
    account::Account,
    bytesrepr::{self, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::Contract,
    package::Package,
    system::auction::{Bid, BidKind, EraInfo, UnbondingPurse, WithdrawPurse},
    AddressableEntity, CLValue, Context, ContractWasm, DeployInfo, EraId, Transfer,
};
pub use type_mismatch::TypeMismatch;

#[allow(clippy::large_enum_variant)]
#[repr(u8)]
enum Tag {
    CLValue = 0,
    Account = 1,
    ContractWasm = 2,
    Contract = 3,
    ContractPackage = 4,
    Transfer = 5,
    DeployInfo = 6,
    EraInfo = 7,
    Bid = 8,
    Withdraw = 9,
    Unbonding = 10,
    AddressableEntity = 11,
    BidKind = 12,
    URef = 13,
}

/// A value stored in Global State.
#[allow(clippy::large_enum_variant)]
#[derive(Eq, PartialEq, Clone, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(with = "serde_helpers::BinarySerHelper")
)]
pub enum StoredValue {
    /// A CLValue.
    CLValue(CLValue),
    /// An account.
    Account(Account),
    /// A contract Wasm.
    ContractWasm(ContractWasm),
    /// A contract.
    Contract(Contract),
    /// A `Package`.
    ContractPackage(Package),
    /// A `Transfer`.
    Transfer(Transfer),
    /// Info about a deploy.
    DeployInfo(DeployInfo),
    /// Info about an era.
    EraInfo(EraInfo),
    /// Variant that stores [`Bid`].
    Bid(Box<Bid>),
    /// Variant that stores withdraw information.
    Withdraw(Vec<WithdrawPurse>),
    /// Unbonding information.
    Unbonding(Vec<UnbondingPurse>),
    /// An `AddressableEntity`.
    AddressableEntity(AddressableEntity),
    /// Variant that stores [`BidKind`].
    BidKind(BidKind),
    /// Variant that stores a [`Context`] and a [`Lifetime`] of the [`crate::URef`].
    URef(Context, Lifetime),
}

impl StoredValue {
    /// Returns a reference to the wrapped `CLValue` if this is a `CLValue` variant.
    pub fn as_cl_value(&self) -> Option<&CLValue> {
        match self {
            StoredValue::CLValue(cl_value) => Some(cl_value),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `Account` if this is an `Account` variant.
    pub fn as_account(&self) -> Option<&Account> {
        match self {
            StoredValue::Account(account) => Some(account),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `ContractWasm` if this is a `ContractWasm` variant.
    pub fn as_contract_wasm(&self) -> Option<&ContractWasm> {
        match self {
            StoredValue::ContractWasm(contract_wasm) => Some(contract_wasm),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `Contract` if this is a `Contract` variant.
    pub fn as_contract(&self) -> Option<&Contract> {
        match self {
            StoredValue::Contract(contract) => Some(contract),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `Package` if this is a `Package` variant.
    pub fn as_contract_package(&self) -> Option<&Package> {
        match self {
            StoredValue::ContractPackage(contract_package) => Some(contract_package),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `Transfer` if this is a `Transfer` variant.
    pub fn as_transfer(&self) -> Option<&Transfer> {
        match self {
            StoredValue::Transfer(transfer) => Some(transfer),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `DeployInfo` if this is a `DeployInfo` variant.
    pub fn as_deploy_info(&self) -> Option<&DeployInfo> {
        match self {
            StoredValue::DeployInfo(deploy_info) => Some(deploy_info),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `EraInfo` if this is an `EraInfo` variant.
    pub fn as_era_info(&self) -> Option<&EraInfo> {
        match self {
            StoredValue::EraInfo(era_info) => Some(era_info),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `Bid` if this is a `Bid` variant.
    pub fn as_bid(&self) -> Option<&Bid> {
        match self {
            StoredValue::Bid(bid) => Some(bid),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped list of `WithdrawPurse`s if this is a `Withdraw` variant.
    pub fn as_withdraw(&self) -> Option<&Vec<WithdrawPurse>> {
        match self {
            StoredValue::Withdraw(withdraw_purses) => Some(withdraw_purses),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped list of `UnbondingPurse`s if this is an `Unbonding`
    /// variant.
    pub fn as_unbonding(&self) -> Option<&Vec<UnbondingPurse>> {
        match self {
            StoredValue::Unbonding(unbonding_purses) => Some(unbonding_purses),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `AddressableEntity` if this is an `AddressableEntity`
    /// variant.
    pub fn as_addressable_entity(&self) -> Option<&AddressableEntity> {
        match self {
            StoredValue::AddressableEntity(entity) => Some(entity),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped `BidKind` if this is a `BidKind` variant.
    pub fn as_bid_kind(&self) -> Option<&BidKind> {
        match self {
            StoredValue::BidKind(bid_kind) => Some(bid_kind),
            _ => None,
        }
    }

    /// Returns the `CLValue` if this is a `CLValue` variant.
    pub fn into_cl_value(self) -> Option<CLValue> {
        match self {
            StoredValue::CLValue(cl_value) => Some(cl_value),
            _ => None,
        }
    }

    /// Returns the `Account` if this is an `Account` variant.
    pub fn into_account(self) -> Option<Account> {
        match self {
            StoredValue::Account(account) => Some(account),
            _ => None,
        }
    }

    /// Returns the `ContractWasm` if this is a `ContractWasm` variant.
    pub fn into_contract_wasm(self) -> Option<ContractWasm> {
        match self {
            StoredValue::ContractWasm(contract_wasm) => Some(contract_wasm),
            _ => None,
        }
    }

    /// Returns the `Contract` if this is a `Contract` variant.
    pub fn into_contract(self) -> Option<Contract> {
        match self {
            StoredValue::Contract(contract) => Some(contract),
            _ => None,
        }
    }

    /// Returns the `Package` if this is a `Package` variant.
    pub fn into_contract_package(self) -> Option<Package> {
        match self {
            StoredValue::ContractPackage(contract_package) => Some(contract_package),
            _ => None,
        }
    }

    /// Returns the `Transfer` if this is a `Transfer` variant.
    pub fn into_transfer(self) -> Option<Transfer> {
        match self {
            StoredValue::Transfer(transfer) => Some(transfer),
            _ => None,
        }
    }

    /// Returns the `DeployInfo` if this is a `DeployInfo` variant.
    pub fn into_deploy_info(self) -> Option<DeployInfo> {
        match self {
            StoredValue::DeployInfo(deploy_info) => Some(deploy_info),
            _ => None,
        }
    }

    /// Returns the `EraInfo` if this is an `EraInfo` variant.
    pub fn into_era_info(self) -> Option<EraInfo> {
        match self {
            StoredValue::EraInfo(era_info) => Some(era_info),
            _ => None,
        }
    }

    /// Returns the `Bid` if this is a `Bid` variant.
    pub fn into_bid(self) -> Option<Bid> {
        match self {
            StoredValue::Bid(bid) => Some(*bid),
            _ => None,
        }
    }

    /// Returns the list of `WithdrawPurse`s if this is a `Withdraw` variant.
    pub fn into_withdraw(self) -> Option<Vec<WithdrawPurse>> {
        match self {
            StoredValue::Withdraw(withdraw_purses) => Some(withdraw_purses),
            _ => None,
        }
    }

    /// Returns the list of `UnbondingPurse`s if this is an `Unbonding` variant.
    pub fn into_unbonding(self) -> Option<Vec<UnbondingPurse>> {
        match self {
            StoredValue::Unbonding(unbonding_purses) => Some(unbonding_purses),
            _ => None,
        }
    }

    /// Returns the `AddressableEntity` if this is an `AddressableEntity` variant.
    pub fn into_addressable_entity(self) -> Option<AddressableEntity> {
        match self {
            StoredValue::AddressableEntity(entity) => Some(entity),
            _ => None,
        }
    }

    /// Returns the `BidKind` if this is a `BidKind` variant.
    pub fn into_bid_kind(self) -> Option<BidKind> {
        match self {
            StoredValue::BidKind(bid_kind) => Some(bid_kind),
            _ => None,
        }
    }

    /// Returns the type name of the [`StoredValue`] enum variant.
    ///
    /// For [`CLValue`] variants it will return the name of the [`CLType`](crate::cl_type::CLType)
    pub fn type_name(&self) -> String {
        match self {
            StoredValue::CLValue(cl_value) => format!("{:?}", cl_value.cl_type()),
            StoredValue::Account(_) => "Account".to_string(),
            StoredValue::ContractWasm(_) => "ContractWasm".to_string(),
            StoredValue::Contract(_) => "Contract".to_string(),
            StoredValue::ContractPackage(_) => "ContractPackage".to_string(),
            StoredValue::Transfer(_) => "Transfer".to_string(),
            StoredValue::DeployInfo(_) => "DeployInfo".to_string(),
            StoredValue::EraInfo(_) => "EraInfo".to_string(),
            StoredValue::Bid(_) => "Bid".to_string(),
            StoredValue::Withdraw(_) => "Withdraw".to_string(),
            StoredValue::Unbonding(_) => "Unbonding".to_string(),
            StoredValue::AddressableEntity(_) => "AddressableEntity".to_string(),
            StoredValue::BidKind(_) => "BidKind".to_string(),
            StoredValue::URef(_, _) => "URef".to_string(),
        }
    }

    fn tag(&self) -> Tag {
        match self {
            StoredValue::CLValue(_) => Tag::CLValue,
            StoredValue::Account(_) => Tag::Account,
            StoredValue::ContractWasm(_) => Tag::ContractWasm,
            StoredValue::Contract(_) => Tag::Contract,
            StoredValue::ContractPackage(_) => Tag::ContractPackage,
            StoredValue::Transfer(_) => Tag::Transfer,
            StoredValue::DeployInfo(_) => Tag::DeployInfo,
            StoredValue::EraInfo(_) => Tag::EraInfo,
            StoredValue::Bid(_) => Tag::Bid,
            StoredValue::Withdraw(_) => Tag::Withdraw,
            StoredValue::Unbonding(_) => Tag::Unbonding,
            StoredValue::AddressableEntity(_) => Tag::AddressableEntity,
            StoredValue::BidKind(_) => Tag::BidKind,
            StoredValue::URef(_, _) => Tag::URef,
        }
    }
}

impl From<CLValue> for StoredValue {
    fn from(value: CLValue) -> StoredValue {
        StoredValue::CLValue(value)
    }
}
impl From<Account> for StoredValue {
    fn from(value: Account) -> StoredValue {
        StoredValue::Account(value)
    }
}

impl From<ContractWasm> for StoredValue {
    fn from(value: ContractWasm) -> StoredValue {
        StoredValue::ContractWasm(value)
    }
}

impl From<Contract> for StoredValue {
    fn from(value: Contract) -> Self {
        StoredValue::Contract(value)
    }
}

impl From<AddressableEntity> for StoredValue {
    fn from(value: AddressableEntity) -> StoredValue {
        StoredValue::AddressableEntity(value)
    }
}
impl From<Package> for StoredValue {
    fn from(value: Package) -> StoredValue {
        StoredValue::ContractPackage(value)
    }
}

impl From<Bid> for StoredValue {
    fn from(bid: Bid) -> StoredValue {
        StoredValue::Bid(Box::new(bid))
    }
}

impl From<BidKind> for StoredValue {
    fn from(bid_kind: BidKind) -> StoredValue {
        StoredValue::BidKind(bid_kind)
    }
}

impl TryFrom<StoredValue> for CLValue {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        let type_name = stored_value.type_name();
        match stored_value {
            StoredValue::CLValue(cl_value) => Ok(cl_value),
            StoredValue::ContractPackage(contract_package) => Ok(CLValue::from_t(contract_package)
                .map_err(|_error| TypeMismatch::new("ContractPackage".to_string(), type_name))?),
            _ => Err(TypeMismatch::new("CLValue".to_string(), type_name)),
        }
    }
}

impl TryFrom<StoredValue> for Account {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        match stored_value {
            StoredValue::Account(account) => Ok(account),
            _ => Err(TypeMismatch::new(
                "Account".to_string(),
                stored_value.type_name(),
            )),
        }
    }
}

impl TryFrom<StoredValue> for ContractWasm {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        match stored_value {
            StoredValue::ContractWasm(contract_wasm) => Ok(contract_wasm),
            _ => Err(TypeMismatch::new(
                "ContractWasm".to_string(),
                stored_value.type_name(),
            )),
        }
    }
}

impl TryFrom<StoredValue> for Contract {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        match stored_value {
            StoredValue::Contract(contract) => Ok(contract),
            _ => Err(TypeMismatch::new(
                "Contract".to_string(),
                stored_value.type_name(),
            )),
        }
    }
}

impl TryFrom<StoredValue> for Package {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        match stored_value {
            StoredValue::ContractPackage(contract_package) => Ok(contract_package),
            _ => Err(TypeMismatch::new(
                "ContractPackage".to_string(),
                stored_value.type_name(),
            )),
        }
    }
}

impl TryFrom<StoredValue> for AddressableEntity {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        match stored_value {
            StoredValue::AddressableEntity(contract) => Ok(contract),
            _ => Err(TypeMismatch::new(
                "AddressableEntity".to_string(),
                stored_value.type_name(),
            )),
        }
    }
}

impl TryFrom<StoredValue> for Transfer {
    type Error = TypeMismatch;

    fn try_from(value: StoredValue) -> Result<Self, Self::Error> {
        match value {
            StoredValue::Transfer(transfer) => Ok(transfer),
            _ => Err(TypeMismatch::new("Transfer".to_string(), value.type_name())),
        }
    }
}

impl TryFrom<StoredValue> for DeployInfo {
    type Error = TypeMismatch;

    fn try_from(value: StoredValue) -> Result<Self, Self::Error> {
        match value {
            StoredValue::DeployInfo(deploy_info) => Ok(deploy_info),
            _ => Err(TypeMismatch::new(
                "DeployInfo".to_string(),
                value.type_name(),
            )),
        }
    }
}

impl TryFrom<StoredValue> for EraInfo {
    type Error = TypeMismatch;

    fn try_from(value: StoredValue) -> Result<Self, Self::Error> {
        match value {
            StoredValue::EraInfo(era_info) => Ok(era_info),
            _ => Err(TypeMismatch::new("EraInfo".to_string(), value.type_name())),
        }
    }
}

impl TryFrom<StoredValue> for Bid {
    type Error = TypeMismatch;

    fn try_from(value: StoredValue) -> Result<Self, Self::Error> {
        match value {
            StoredValue::Bid(bid) => Ok(*bid),
            _ => Err(TypeMismatch::new("Bid".to_string(), value.type_name())),
        }
    }
}

impl TryFrom<StoredValue> for BidKind {
    type Error = TypeMismatch;

    fn try_from(value: StoredValue) -> Result<Self, Self::Error> {
        match value {
            StoredValue::BidKind(bid_kind) => Ok(bid_kind),
            _ => Err(TypeMismatch::new("BidKind".to_string(), value.type_name())),
        }
    }
}

impl ToBytes for StoredValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                StoredValue::CLValue(cl_value) => cl_value.serialized_length(),
                StoredValue::Account(account) => account.serialized_length(),
                StoredValue::ContractWasm(contract_wasm) => contract_wasm.serialized_length(),
                StoredValue::Contract(contract_header) => contract_header.serialized_length(),
                StoredValue::ContractPackage(contract_package) => {
                    contract_package.serialized_length()
                }
                StoredValue::Transfer(transfer) => transfer.serialized_length(),
                StoredValue::DeployInfo(deploy_info) => deploy_info.serialized_length(),
                StoredValue::EraInfo(era_info) => era_info.serialized_length(),
                StoredValue::Bid(bid) => bid.serialized_length(),
                StoredValue::Withdraw(withdraw_purses) => withdraw_purses.serialized_length(),
                StoredValue::Unbonding(unbonding_purses) => unbonding_purses.serialized_length(),
                StoredValue::AddressableEntity(entity) => entity.serialized_length(),
                StoredValue::BidKind(bid_kind) => bid_kind.serialized_length(),
                StoredValue::URef(context, lifetime) => {
                    context.serialized_length() + lifetime.serialized_length()
                }
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag() as u8);
        match self {
            StoredValue::CLValue(cl_value) => cl_value.write_bytes(writer)?,
            StoredValue::Account(account) => account.write_bytes(writer)?,
            StoredValue::ContractWasm(contract_wasm) => contract_wasm.write_bytes(writer)?,
            StoredValue::Contract(contract_header) => contract_header.write_bytes(writer)?,
            StoredValue::ContractPackage(contract_package) => {
                contract_package.write_bytes(writer)?
            }
            StoredValue::Transfer(transfer) => transfer.write_bytes(writer)?,
            StoredValue::DeployInfo(deploy_info) => deploy_info.write_bytes(writer)?,
            StoredValue::EraInfo(era_info) => era_info.write_bytes(writer)?,
            StoredValue::Bid(bid) => bid.write_bytes(writer)?,
            StoredValue::Withdraw(unbonding_purses) => unbonding_purses.write_bytes(writer)?,
            StoredValue::Unbonding(unbonding_purses) => unbonding_purses.write_bytes(writer)?,
            StoredValue::AddressableEntity(entity) => entity.write_bytes(writer)?,
            StoredValue::BidKind(bid_kind) => bid_kind.write_bytes(writer)?,
            StoredValue::URef(context, lifetime) => {
                context.write_bytes(writer)?;
                lifetime.write_bytes(writer)?;
            }
        };
        Ok(())
    }
}

impl FromBytes for StoredValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            tag if tag == Tag::CLValue as u8 => CLValue::from_bytes(remainder)
                .map(|(cl_value, remainder)| (StoredValue::CLValue(cl_value), remainder)),
            tag if tag == Tag::Account as u8 => Account::from_bytes(remainder)
                .map(|(account, remainder)| (StoredValue::Account(account), remainder)),
            tag if tag == Tag::ContractWasm as u8 => {
                ContractWasm::from_bytes(remainder).map(|(contract_wasm, remainder)| {
                    (StoredValue::ContractWasm(contract_wasm), remainder)
                })
            }
            tag if tag == Tag::ContractPackage as u8 => {
                Package::from_bytes(remainder).map(|(contract_package, remainder)| {
                    (StoredValue::ContractPackage(contract_package), remainder)
                })
            }
            tag if tag == Tag::Contract as u8 => Contract::from_bytes(remainder)
                .map(|(contract, remainder)| (StoredValue::Contract(contract), remainder)),
            tag if tag == Tag::Transfer as u8 => Transfer::from_bytes(remainder)
                .map(|(transfer, remainder)| (StoredValue::Transfer(transfer), remainder)),
            tag if tag == Tag::DeployInfo as u8 => DeployInfo::from_bytes(remainder)
                .map(|(deploy_info, remainder)| (StoredValue::DeployInfo(deploy_info), remainder)),
            tag if tag == Tag::EraInfo as u8 => EraInfo::from_bytes(remainder)
                .map(|(deploy_info, remainder)| (StoredValue::EraInfo(deploy_info), remainder)),
            tag if tag == Tag::Bid as u8 => Bid::from_bytes(remainder)
                .map(|(bid, remainder)| (StoredValue::Bid(Box::new(bid)), remainder)),
            tag if tag == Tag::BidKind as u8 => BidKind::from_bytes(remainder)
                .map(|(bid_kind, remainder)| (StoredValue::BidKind(bid_kind), remainder)),
            tag if tag == Tag::Withdraw as u8 => {
                Vec::<WithdrawPurse>::from_bytes(remainder).map(|(withdraw_purses, remainder)| {
                    (StoredValue::Withdraw(withdraw_purses), remainder)
                })
            }
            tag if tag == Tag::Unbonding as u8 => {
                Vec::<UnbondingPurse>::from_bytes(remainder).map(|(unbonding_purses, remainder)| {
                    (StoredValue::Unbonding(unbonding_purses), remainder)
                })
            }
            tag if tag == Tag::AddressableEntity as u8 => AddressableEntity::from_bytes(remainder)
                .map(|(entity, remainder)| (StoredValue::AddressableEntity(entity), remainder)),
            tag if tag == Tag::URef as u8 => {
                let (context, remainder) = Context::from_bytes(remainder)?;
                let (lifetime, remainder) = Lifetime::from_bytes(remainder)?;
                Ok((StoredValue::URef(context, lifetime), remainder))
            }
            _ => Err(Error::Formatting),
        }
    }
}

mod serde_helpers {
    use super::*;

    #[derive(Serialize)]
    pub(super) enum BinarySerHelper<'a> {
        /// A CLValue.
        CLValue(&'a CLValue),
        /// An account.
        Account(&'a Account),
        /// A contract Wasm.
        ContractWasm(&'a ContractWasm),
        /// A contract.
        Contract(&'a Contract),
        /// A `Package`.
        ContractPackage(&'a Package),
        /// A `Transfer`.
        Transfer(&'a Transfer),
        /// Info about a deploy.
        DeployInfo(&'a DeployInfo),
        /// Info about an era.
        EraInfo(&'a EraInfo),
        /// Variant that stores [`Bid`].
        Bid(&'a Bid),
        /// Variant that stores withdraw information.
        Withdraw(&'a Vec<WithdrawPurse>),
        /// Unbonding information.
        Unbonding(&'a Vec<UnbondingPurse>),
        /// An `AddressableEntity`.
        AddressableEntity(&'a AddressableEntity),
        /// Variant that stores [`BidKind`].
        BidKind(&'a BidKind),
        /// Variant that stores a [`Context`] and a [`Lifetime`] of the [`crate::URef`].
        URef(&'a Context, &'a Lifetime),
    }

    #[derive(Deserialize)]
    pub(super) enum BinaryDeserHelper {
        /// A CLValue.
        CLValue(CLValue),
        /// An account.
        Account(Account),
        /// A contract Wasm.
        ContractWasm(ContractWasm),
        /// A contract.
        Contract(Contract),
        /// A `Package`.
        ContractPackage(Package),
        /// A `Transfer`.
        Transfer(Transfer),
        /// Info about a deploy.
        DeployInfo(DeployInfo),
        /// Info about an era.
        EraInfo(EraInfo),
        /// Variant that stores [`Bid`].
        Bid(Box<Bid>),
        /// Variant that stores withdraw information.
        Withdraw(Vec<WithdrawPurse>),
        /// Unbonding information.
        Unbonding(Vec<UnbondingPurse>),
        /// An `AddressableEntity`.
        AddressableEntity(AddressableEntity),
        /// Variant that stores [`BidKind`].
        BidKind(BidKind),
        /// Variant that stores a [`Context`] and a [`Lifetime`] of the [`crate::URef`].
        URef(Context, Lifetime),
    }

    impl<'a> From<&'a StoredValue> for BinarySerHelper<'a> {
        fn from(stored_value: &'a StoredValue) -> Self {
            match stored_value {
                StoredValue::CLValue(payload) => BinarySerHelper::CLValue(payload),
                StoredValue::Account(payload) => BinarySerHelper::Account(payload),
                StoredValue::ContractWasm(payload) => BinarySerHelper::ContractWasm(payload),
                StoredValue::Contract(payload) => BinarySerHelper::Contract(payload),
                StoredValue::ContractPackage(payload) => BinarySerHelper::ContractPackage(payload),
                StoredValue::Transfer(payload) => BinarySerHelper::Transfer(payload),
                StoredValue::DeployInfo(payload) => BinarySerHelper::DeployInfo(payload),
                StoredValue::EraInfo(payload) => BinarySerHelper::EraInfo(payload),
                StoredValue::Bid(payload) => BinarySerHelper::Bid(payload),
                StoredValue::Withdraw(payload) => BinarySerHelper::Withdraw(payload),
                StoredValue::Unbonding(payload) => BinarySerHelper::Unbonding(payload),
                StoredValue::AddressableEntity(payload) => {
                    BinarySerHelper::AddressableEntity(payload)
                }
                StoredValue::BidKind(payload) => BinarySerHelper::BidKind(payload),
                StoredValue::URef(context, lifetime) => BinarySerHelper::URef(context, lifetime),
            }
        }
    }

    impl From<BinaryDeserHelper> for StoredValue {
        fn from(helper: BinaryDeserHelper) -> Self {
            match helper {
                BinaryDeserHelper::CLValue(payload) => StoredValue::CLValue(payload),
                BinaryDeserHelper::Account(payload) => StoredValue::Account(payload),
                BinaryDeserHelper::ContractWasm(payload) => StoredValue::ContractWasm(payload),
                BinaryDeserHelper::Contract(payload) => StoredValue::Contract(payload),
                BinaryDeserHelper::ContractPackage(payload) => {
                    StoredValue::ContractPackage(payload)
                }
                BinaryDeserHelper::Transfer(payload) => StoredValue::Transfer(payload),
                BinaryDeserHelper::DeployInfo(payload) => StoredValue::DeployInfo(payload),
                BinaryDeserHelper::EraInfo(payload) => StoredValue::EraInfo(payload),
                BinaryDeserHelper::Bid(bid) => StoredValue::Bid(bid),
                BinaryDeserHelper::Withdraw(payload) => StoredValue::Withdraw(payload),
                BinaryDeserHelper::Unbonding(payload) => StoredValue::Unbonding(payload),
                BinaryDeserHelper::AddressableEntity(payload) => {
                    StoredValue::AddressableEntity(payload)
                }
                BinaryDeserHelper::BidKind(payload) => StoredValue::BidKind(payload),
                BinaryDeserHelper::URef(context, lifetime) => StoredValue::URef(context, lifetime),
            }
        }
    }
}

impl Serialize for StoredValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serde_helpers::BinarySerHelper::from(self).serialize(serializer)
        } else {
            let bytes = self
                .to_bytes()
                .map_err(|error| ser::Error::custom(format!("{:?}", error)))?;
            ByteBuf::from(bytes).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for StoredValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let json_helper = serde_helpers::BinaryDeserHelper::deserialize(deserializer)?;
            Ok(StoredValue::from(json_helper))
        } else {
            let bytes = ByteBuf::deserialize(deserializer)?.into_vec();
            bytesrepr::deserialize::<StoredValue>(bytes)
                .map_err(|error| de::Error::custom(format!("{:?}", error)))
        }
    }
}

/// Lifetime of a value stored behind a [`crate::URef`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum Lifetime {
    /// Indefinite lifetime.
    Indefinite,
    /// Finite lifetime ending at a specific [`EraId`].
    Finite(EraId),
}

impl ToBytes for Lifetime {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Lifetime::Indefinite => 0,
                Lifetime::Finite(era_id) => era_id.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            Lifetime::Indefinite => writer.push(0),
            Lifetime::Finite(era_id) => {
                writer.push(1);
                era_id.write_bytes(writer)?;
            }
        }
        Ok(())
    }
}

impl FromBytes for Lifetime {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        match u8::from_bytes(bytes)? {
            (0, remainder) => Ok((Lifetime::Indefinite, remainder)),
            (1, remainder) => {
                let (era_id, remainder) = EraId::from_bytes(remainder)?;
                Ok((Lifetime::Finite(era_id), remainder))
            }
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn serialization_roundtrip(v in gens::stored_value_arb()) {
            bytesrepr::test_serialization_roundtrip(&v);
        }
    }
}
