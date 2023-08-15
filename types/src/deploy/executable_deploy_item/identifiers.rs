use alloc::string::String;

#[cfg(doc)]
use super::ExecutableDeployItem;
use crate::{
    package::{ContractPackageHash, ContractVersion},
    ContractHash,
};

/// Identifier for an [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExecutableDeployItemIdentifier {
    /// The deploy item is of the type [`ExecutableDeployItem::ModuleBytes`]
    Module,
    /// The deploy item is a variation of a stored contract.
    Contract(ContractIdentifier),
    /// The deploy item is a variation of a stored contract package.
    Package(ContractPackageIdentifier),
    /// The deploy item is a native transfer.
    Transfer,
}

/// Identifier for the contract object within an [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ContractIdentifier {
    /// The contract object within the deploy item is identified by name.
    Name(String),
    /// The contract object within the deploy item is identified by its hash.
    Hash(ContractHash),
}

/// Identifier for the contract package object within an [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ContractPackageIdentifier {
    /// The stored contract package within the deploy item is identified by name.
    Name {
        /// Name of the contract package.
        name: String,
        /// The version specified in the deploy item.
        version: Option<ContractVersion>,
    },
    /// The stored contract package within the deploy item is identified by its hash.
    Hash {
        /// Hash of the contract package.
        contract_package_hash: ContractPackageHash,
        /// The version specified in the deploy item.
        version: Option<ContractVersion>,
    },
}

impl ContractPackageIdentifier {
    /// Returns the version of the contract package specified in the deploy item.
    pub fn version(&self) -> Option<ContractVersion> {
        match self {
            ContractPackageIdentifier::Name { version, .. }
            | ContractPackageIdentifier::Hash { version, .. } => *version,
        }
    }
}
