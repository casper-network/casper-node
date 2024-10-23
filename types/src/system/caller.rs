pub mod call_stack_elements;

use alloc::{collections::BTreeMap, vec::Vec};

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    package::PackageHash,
    CLType, CLTyped, CLValue, CLValueError, EntityAddr, HashAddr,
};

use crate::{
    bytesrepr::Error,
    contracts::{ContractHash, ContractPackageHash},
};
pub use call_stack_elements::CallStackElement;

/// Tag representing variants of CallerTag for purposes of serialization.
#[derive(FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub enum CallerTag {
    /// Initiator tag.
    Initiator = 0,
    /// Entity tag.
    Entity,
    /// Smart contract tag.
    SmartContract,
}

const ACCOUNT: u8 = 0;
const PACKAGE: u8 = 1;
const CONTRACT_PACKAGE: u8 = 2;
const ENTITY: u8 = 3;
const CONTRACT: u8 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerInfo {
    kind: u8,
    fields: BTreeMap<u8, CLValue>,
}

impl CLTyped for CallerInfo {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl CallerInfo {
    pub fn kind(&self) -> u8 {
        self.kind
    }

    pub fn get_field_by_index(&self, index: u8) -> Option<&CLValue> {
        self.fields.get(&index)
    }
}

impl ToBytes for CallerInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.kind.to_bytes()?);
        result.append(&mut self.fields.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH + self.fields.serialized_length()
    }
}

impl FromBytes for CallerInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (kind, remainder) = u8::from_bytes(bytes)?;
        let (fields, remainder) = BTreeMap::from_bytes(remainder)?;
        Ok((CallerInfo { kind, fields }, remainder))
    }
}

impl TryFrom<Caller> for CallerInfo {
    type Error = CLValueError;

    fn try_from(value: Caller) -> Result<Self, Self::Error> {
        match value {
            Caller::Initiator { account_hash } => {
                let kind = ACCOUNT;

                let mut ret = BTreeMap::new();
                ret.insert(ACCOUNT, CLValue::from_t(Some(account_hash))?);
                ret.insert(PACKAGE, CLValue::from_t(Option::<PackageHash>::None)?);
                ret.insert(
                    CONTRACT_PACKAGE,
                    CLValue::from_t(Option::<ContractPackageHash>::None)?,
                );
                ret.insert(ENTITY, CLValue::from_t(Option::<EntityAddr>::None)?);
                ret.insert(CONTRACT, CLValue::from_t(Option::<ContractHash>::None)?);
                Ok(CallerInfo { kind, fields: ret })
            }
            Caller::Entity {
                package_hash,
                entity_addr,
            } => {
                let kind = ENTITY;

                let mut ret = BTreeMap::new();
                ret.insert(ACCOUNT, CLValue::from_t(Option::<AccountHash>::None)?);
                ret.insert(PACKAGE, CLValue::from_t(Some(package_hash))?);
                ret.insert(
                    CONTRACT_PACKAGE,
                    CLValue::from_t(Option::<ContractPackageHash>::None)?,
                );
                ret.insert(ENTITY, CLValue::from_t(Some(entity_addr))?);
                ret.insert(CONTRACT, CLValue::from_t(Option::<ContractHash>::None)?);
                Ok(CallerInfo { kind, fields: ret })
            }
            Caller::SmartContract {
                contract_package_hash,
                contract_hash,
            } => {
                let kind = CONTRACT;

                let mut ret = BTreeMap::new();
                ret.insert(ACCOUNT, CLValue::from_t(Option::<AccountHash>::None)?);
                ret.insert(PACKAGE, CLValue::from_t(Option::<PackageHash>::None)?);
                ret.insert(
                    CONTRACT_PACKAGE,
                    CLValue::from_t(Some(contract_package_hash))?,
                );

                ret.insert(ENTITY, CLValue::from_t(Option::<EntityAddr>::None)?);
                ret.insert(CONTRACT, CLValue::from_t(Some(contract_hash))?);
                Ok(CallerInfo { kind, fields: ret })
            }
        }
    }
}

/// Identity of a calling entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caller {
    /// Initiator (calling account)
    Initiator {
        /// The account hash of the caller
        account_hash: AccountHash,
    },
    /// Entity (smart contract / system contract)
    Entity {
        /// The package hash
        package_hash: PackageHash,
        /// The entity addr.
        entity_addr: EntityAddr,
    },
    SmartContract {
        /// The contract package hash.
        contract_package_hash: ContractPackageHash,
        /// The contract hash.
        contract_hash: ContractHash,
    },
}

impl Caller {
    /// Creates a [`Caller::Initiator`]. This represents a call into session code, and
    /// should only ever happen once in a call stack.
    pub fn initiator(account_hash: AccountHash) -> Self {
        Caller::Initiator { account_hash }
    }

    /// Creates a [`'Caller::Entity`]. This represents a call into a contract with
    /// `EntryPointType::Called`.
    pub fn entity(package_hash: PackageHash, entity_addr: EntityAddr) -> Self {
        Caller::Entity {
            package_hash,
            entity_addr,
        }
    }

    pub fn smart_contract(
        contract_package_hash: ContractPackageHash,
        contract_hash: ContractHash,
    ) -> Self {
        Caller::SmartContract {
            contract_package_hash,
            contract_hash,
        }
    }

    /// Gets the tag from self.
    pub fn tag(&self) -> CallerTag {
        match self {
            Caller::Initiator { .. } => CallerTag::Initiator,
            Caller::Entity { .. } => CallerTag::Entity,
            Caller::SmartContract { .. } => CallerTag::SmartContract,
        }
    }

    /// Gets the [`HashAddr`] for both stored session and stored contract variants.
    pub fn contract_hash(&self) -> Option<HashAddr> {
        match self {
            Caller::Initiator { .. } => None,
            Caller::Entity { entity_addr, .. } => Some(entity_addr.value()),
            Caller::SmartContract { contract_hash, .. } => Some(contract_hash.value()),
        }
    }
}

impl ToBytes for Caller {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.push(self.tag() as u8);
        match self {
            Caller::Initiator { account_hash } => result.append(&mut account_hash.to_bytes()?),

            Caller::Entity {
                package_hash,
                entity_addr,
            } => {
                result.append(&mut package_hash.to_bytes()?);
                result.append(&mut entity_addr.to_bytes()?);
            }
            Caller::SmartContract {
                contract_package_hash,
                contract_hash,
            } => {
                result.append(&mut contract_package_hash.to_bytes()?);
                result.append(&mut contract_hash.to_bytes()?);
            }
        };
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Caller::Initiator { account_hash } => account_hash.serialized_length(),
                Caller::Entity {
                    package_hash,
                    entity_addr,
                } => package_hash.serialized_length() + entity_addr.serialized_length(),
                Caller::SmartContract {
                    contract_package_hash,
                    contract_hash,
                } => contract_package_hash.serialized_length() + contract_hash.serialized_length(),
            }
    }
}

impl FromBytes for Caller {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        let tag = CallerTag::from_u8(tag).ok_or(bytesrepr::Error::Formatting)?;
        match tag {
            CallerTag::Initiator => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((Caller::Initiator { account_hash }, remainder))
            }
            CallerTag::Entity => {
                let (package_hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (entity_addr, remainder) = EntityAddr::from_bytes(remainder)?;
                Ok((
                    Caller::Entity {
                        package_hash,
                        entity_addr,
                    },
                    remainder,
                ))
            }
            CallerTag::SmartContract => {
                let (contract_package_hash, remainder) =
                    ContractPackageHash::from_bytes(remainder)?;
                let (contract_hash, remainder) = ContractHash::from_bytes(remainder)?;
                Ok((
                    Caller::SmartContract {
                        contract_package_hash,
                        contract_hash,
                    },
                    remainder,
                ))
            }
        }
    }
}

impl CLTyped for Caller {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl From<&Caller> for CallStackElement {
    fn from(caller: &Caller) -> Self {
        match caller {
            Caller::Initiator { account_hash } => CallStackElement::Session {
                account_hash: *account_hash,
            },
            Caller::Entity {
                package_hash,
                entity_addr: entity_hash,
            } => CallStackElement::StoredContract {
                contract_package_hash: ContractPackageHash::new(package_hash.value()),
                contract_hash: ContractHash::new(entity_hash.value()),
            },
            Caller::SmartContract {
                contract_package_hash,
                contract_hash,
            } => CallStackElement::StoredContract {
                contract_package_hash: *contract_package_hash,
                contract_hash: *contract_hash,
            },
        }
    }
}
