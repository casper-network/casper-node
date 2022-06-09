mod type_mismatch;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::{convert::TryFrom, fmt::Debug};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};
use serde_bytes::ByteBuf;

use crate::{
    account::Account,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::ContractPackage,
    system::auction::{Bid, EraInfo, UnbondingPurse, WithdrawPurse},
    CLValue, Contract, ContractWasm, DeployInfo, Transfer,
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
}

#[allow(clippy::large_enum_variant)]
#[derive(Eq, PartialEq, Clone, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
/// StoredValue represents all possible variants of values stored in Global State.
pub enum StoredValue {
    /// Variant that stores [`CLValue`].
    CLValue(CLValue),
    /// Variant that stores [`Account`].
    Account(Account),
    /// Variant that stores [`ContractWasm`].
    ContractWasm(ContractWasm),
    /// Variant that stores [`Contract`].
    Contract(Contract),
    /// Variant that stores [`ContractPackage`].
    ContractPackage(ContractPackage),
    /// Variant that stores [`Transfer`].
    Transfer(Transfer),
    /// Variant that stores [`DeployInfo`].
    DeployInfo(DeployInfo),
    /// Variant that stores [`EraInfo`].
    EraInfo(EraInfo),
    /// Variant that stores [`Bid`].
    Bid(Box<Bid>),
    /// Variant that stores withdraw information.
    Withdraw(Vec<WithdrawPurse>),
    /// Variant that stores unbonding information.
    Unbonding(Vec<UnbondingPurse>),
}

impl StoredValue {
    /// Returns a wrapped [`CLValue`] if this is a `CLValue` variant.
    pub fn as_cl_value(&self) -> Option<&CLValue> {
        match self {
            StoredValue::CLValue(cl_value) => Some(cl_value),
            _ => None,
        }
    }

    /// Returns a wrapped [`Account`] if this is an `Account` variant.
    pub fn as_account(&self) -> Option<&Account> {
        match self {
            StoredValue::Account(account) => Some(account),
            _ => None,
        }
    }

    /// Returns a wrapped [`Contract`] if this is a `Contract` variant.
    pub fn as_contract(&self) -> Option<&Contract> {
        match self {
            StoredValue::Contract(contract) => Some(contract),
            _ => None,
        }
    }

    /// Returns a wrapped [`ContractWasm`] if this is a `ContractWasm` variant.
    pub fn as_contract_wasm(&self) -> Option<&ContractWasm> {
        match self {
            StoredValue::ContractWasm(contract_wasm) => Some(contract_wasm),
            _ => None,
        }
    }

    /// Returns a wrapped [`ContractPackage`] if this is a `ContractPackage` variant.
    pub fn as_contract_package(&self) -> Option<&ContractPackage> {
        match self {
            StoredValue::ContractPackage(contract_package) => Some(contract_package),
            _ => None,
        }
    }

    /// Returns a wrapped [`DeployInfo`] if this is a `DeployInfo` variant.
    pub fn as_deploy_info(&self) -> Option<&DeployInfo> {
        match self {
            StoredValue::DeployInfo(deploy_info) => Some(deploy_info),
            _ => None,
        }
    }

    /// Returns a wrapped [`EraInfo`] if this is a `EraInfo` variant.
    pub fn as_era_info(&self) -> Option<&EraInfo> {
        match self {
            StoredValue::EraInfo(era_info) => Some(era_info),
            _ => None,
        }
    }

    /// Returns a wrapped [`Bid`] if this is a `Bid` variant.
    pub fn as_bid(&self) -> Option<&Bid> {
        match self {
            StoredValue::Bid(bid) => Some(bid),
            _ => None,
        }
    }

    /// Returns a wrapped list of [`WithdrawPurse`]s if this is a `Withdraw` variant.
    pub fn as_withdraw(&self) -> Option<&Vec<WithdrawPurse>> {
        match self {
            StoredValue::Withdraw(withdraw_purses) => Some(withdraw_purses),
            _ => None,
        }
    }

    /// Returns a wrapped list of [`UnbondingPurse`]s if this is a `Unbonding` variant.
    pub fn as_unbonding(&self) -> Option<&Vec<UnbondingPurse>> {
        match self {
            StoredValue::Unbonding(unbonding_purses) => Some(unbonding_purses),
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
    fn from(value: Contract) -> StoredValue {
        StoredValue::Contract(value)
    }
}
impl From<ContractPackage> for StoredValue {
    fn from(value: ContractPackage) -> StoredValue {
        StoredValue::ContractPackage(value)
    }
}
impl From<Bid> for StoredValue {
    fn from(bid: Bid) -> StoredValue {
        StoredValue::Bid(Box::new(bid))
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

impl TryFrom<StoredValue> for ContractPackage {
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

impl ToBytes for StoredValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        let (tag, mut serialized_data) = match self {
            StoredValue::CLValue(cl_value) => (Tag::CLValue, cl_value.to_bytes()?),
            StoredValue::Account(account) => (Tag::Account, account.to_bytes()?),
            StoredValue::ContractWasm(contract_wasm) => {
                (Tag::ContractWasm, contract_wasm.to_bytes()?)
            }
            StoredValue::Contract(contract_header) => (Tag::Contract, contract_header.to_bytes()?),
            StoredValue::ContractPackage(contract_package) => {
                (Tag::ContractPackage, contract_package.to_bytes()?)
            }
            StoredValue::Transfer(transfer) => (Tag::Transfer, transfer.to_bytes()?),
            StoredValue::DeployInfo(deploy_info) => (Tag::DeployInfo, deploy_info.to_bytes()?),
            StoredValue::EraInfo(era_info) => (Tag::EraInfo, era_info.to_bytes()?),
            StoredValue::Bid(bid) => (Tag::Bid, bid.to_bytes()?),
            StoredValue::Withdraw(withdraw_purses) => (Tag::Withdraw, withdraw_purses.to_bytes()?),
            StoredValue::Unbonding(unbonding_purses) => {
                (Tag::Unbonding, unbonding_purses.to_bytes()?)
            }
        };
        result.push(tag as u8);
        result.append(&mut serialized_data);
        Ok(result)
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
        };
        Ok(())
    }
}

impl FromBytes for StoredValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
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
                ContractPackage::from_bytes(remainder).map(|(contract_package, remainder)| {
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
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Serialize for StoredValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The JSON representation of a StoredValue is just its bytesrepr
        // While this makes it harder to inspect, it makes deterministic representation simple.
        let bytes = self
            .to_bytes()
            .map_err(|error| ser::Error::custom(format!("{:?}", error)))?;
        ByteBuf::from(bytes).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StoredValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = ByteBuf::deserialize(deserializer)?.into_vec();
        bytesrepr::deserialize::<StoredValue>(bytes)
            .map_err(|error| de::Error::custom(format!("{:?}", error)))
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
