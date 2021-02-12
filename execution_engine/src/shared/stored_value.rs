use std::{
    convert::TryFrom,
    fmt::{self, Debug, Formatter},
};

use serde::{
    de::{self, SeqAccess, Visitor},
    ser::{Error, SerializeSeq},
    Deserialize, Deserializer, Serialize, Serializer,
};

use casper_types::{
    auction::EraInfo,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::ContractPackage,
    CLValue, Contract, ContractWasm, DeployInfo, Transfer,
};

use crate::shared::{account::Account, TypeMismatch};

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
}

#[derive(Eq, PartialEq, Clone, Debug)]
pub enum StoredValue {
    CLValue(CLValue),
    Account(Account),
    ContractWasm(ContractWasm),
    Contract(Contract),
    ContractPackage(ContractPackage),
    Transfer(Transfer),
    DeployInfo(DeployInfo),
    EraInfo(EraInfo),
}

impl StoredValue {
    pub fn as_cl_value(&self) -> Option<&CLValue> {
        match self {
            StoredValue::CLValue(cl_value) => Some(cl_value),
            _ => None,
        }
    }

    pub fn as_account(&self) -> Option<&Account> {
        match self {
            StoredValue::Account(account) => Some(account),
            _ => None,
        }
    }

    pub fn as_contract(&self) -> Option<&Contract> {
        match self {
            StoredValue::Contract(contract) => Some(contract),
            _ => None,
        }
    }

    pub fn as_contract_wasm(&self) -> Option<&ContractWasm> {
        match self {
            StoredValue::ContractWasm(contract_wasm) => Some(contract_wasm),
            _ => None,
        }
    }

    pub fn as_contract_package(&self) -> Option<&ContractPackage> {
        match self {
            StoredValue::ContractPackage(contract_package) => Some(&contract_package),
            _ => None,
        }
    }

    pub fn as_deploy_info(&self) -> Option<&DeployInfo> {
        match self {
            StoredValue::DeployInfo(deploy_info) => Some(deploy_info),
            _ => None,
        }
    }

    pub fn as_era_info(&self) -> Option<&EraInfo> {
        match self {
            StoredValue::EraInfo(era_info) => Some(era_info),
            _ => None,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            StoredValue::CLValue(cl_value) => format!("{:?}", cl_value.cl_type()),
            StoredValue::Account(_) => "Account".to_string(),
            StoredValue::ContractWasm(_) => "Contract".to_string(),
            StoredValue::Contract(_) => "Contract".to_string(),
            StoredValue::ContractPackage(_) => "ContractPackage".to_string(),
            StoredValue::Transfer(_) => "Transfer".to_string(),
            StoredValue::DeployInfo(_) => "DeployInfo".to_string(),
            StoredValue::EraInfo(_) => "EraInfo".to_string(),
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

impl TryFrom<StoredValue> for CLValue {
    type Error = TypeMismatch;

    fn try_from(stored_value: StoredValue) -> Result<Self, Self::Error> {
        match stored_value {
            StoredValue::CLValue(cl_value) => Ok(cl_value),
            _ => Err(TypeMismatch::new(
                "CLValue".to_string(),
                stored_value.type_name(),
            )),
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
            }
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
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Serialize for StoredValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // The JSON representation of a StoredValue is just its bytesrepr
        // While this makes it harder to inspect, it makes deterministic representation simple.
        let bytes = self
            .to_bytes()
            .map_err(|err| S::Error::custom(format!("{:?}", err)))?;

        let number_of_bytes = bytes.len();
        let mut seq = serializer.serialize_seq(Some(number_of_bytes))?;

        // First push the number of bytes the StoredValue takes
        seq.serialize_element(&number_of_bytes)?;
        for byte in bytes {
            seq.serialize_element(&byte)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for StoredValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StoredValueDeserializer;

        impl<'de> Visitor<'de> for StoredValueDeserializer {
            type Value = StoredValue;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("Serialized representation of StoredValue in bytes.")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let number_of_bytes: usize = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::custom("Did not have leading number of bytes"))?;
                let mut bytes: Vec<u8> = Vec::with_capacity(number_of_bytes);
                let mut count: usize = 0;
                while let Some(byte) = seq.next_element()? {
                    count += 1;
                    if count > number_of_bytes {
                        return Err(de::Error::custom(format!(
                            "Serialization should have {} bytes but exceeds this",
                            number_of_bytes
                        )));
                    }
                    bytes.push(byte);
                }
                if count < number_of_bytes {
                    return Err(de::Error::custom(format!(
                        "Serialization should have {} bytes but only has {}",
                        number_of_bytes, count
                    )));
                }
                let (stored_value, additional_bytes) = StoredValue::from_bytes(&bytes)
                    .map_err(|err| de::Error::custom(format!("{:?}", err)))?;
                if !additional_bytes.is_empty() {
                    return Err(de::Error::custom(format!(
                        "Parsed stored value as well as unexpected additional bytes.\n\
                         \n\
                         Stored value: {:?}\n\
                         \n\
                         Unexpected additional bytes: {:?}",
                        stored_value, additional_bytes
                    )));
                }
                Ok(stored_value)
            }
        }
        deserializer.deserialize_seq(StoredValueDeserializer)
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use casper_types::gens::cl_value_arb;

    use super::StoredValue;
    use crate::shared::account::gens::account_arb;
    use casper_types::gens::{contract_arb, contract_package_arb, contract_wasm_arb};

    pub fn stored_value_arb() -> impl Strategy<Value = StoredValue> {
        prop_oneof![
            cl_value_arb().prop_map(StoredValue::CLValue),
            account_arb().prop_map(StoredValue::Account),
            contract_package_arb().prop_map(StoredValue::ContractPackage),
            contract_arb().prop_map(StoredValue::Contract),
            contract_wasm_arb().prop_map(StoredValue::ContractWasm),
        ]
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use super::*;

    proptest! {
        #[test]
        fn serialization_roundtrip(v in gens::stored_value_arb()) {
            bytesrepr::test_serialization_roundtrip(&v);
        }
    }
}
