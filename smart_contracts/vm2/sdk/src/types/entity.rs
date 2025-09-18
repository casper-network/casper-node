use crate::{
    abi::{CasperABI, EnumVariant},
    compat::types::{CLType, CLTyped},
    serializers::borsh::{BorshDeserialize, BorshSerialize},
    types::Address,
};

/// Enum representing either an account or a contract.
#[derive(
    BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub enum Entity {
    Account([u8; 32]),
    Contract([u8; 32]),
}

impl CLTyped for Entity {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl Entity {
    /// Get the tag of the entity.
    #[must_use]
    pub fn tag(&self) -> u32 {
        match self {
            Entity::Account(_) => 0,
            Entity::Contract(_) => 1,
        }
    }

    #[must_use]
    pub fn from_parts(tag: u32, address: [u8; 32]) -> Option<Self> {
        match tag {
            0 => Some(Self::Account(address)),
            1 => Some(Self::Contract(address)),
            _ => None,
        }
    }

    #[must_use]
    pub fn address(&self) -> &Address {
        match self {
            Entity::Account(addr) | Entity::Contract(addr) => addr,
        }
    }

    #[must_use]
    pub fn is_account(&self) -> bool {
        match self {
            Entity::Account(_) => true,
            Entity::Contract(_) => false,
        }
    }

    #[must_use]
    pub fn is_contract(&self) -> bool {
        match self {
            Entity::Account(_) => false,
            Entity::Contract(_) => true,
        }
    }
}

impl CasperABI for Entity {
    fn populate_definitions(definitions: &mut crate::abi::Definitions) {
        definitions.populate_one::<[u8; 32]>();
    }

    fn declaration() -> crate::abi::Declaration {
        "Entity".into()
    }

    fn definition() -> crate::abi::Definition {
        crate::abi::Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "Account".into(),
                    discriminant: 0,
                    decl: <[u8; 32] as CasperABI>::declaration(),
                },
                EnumVariant {
                    name: "Contract".into(),
                    discriminant: 1,
                    decl: <[u8; 32] as CasperABI>::declaration(),
                },
            ],
        }
    }
}
