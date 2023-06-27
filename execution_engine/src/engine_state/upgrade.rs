//! Support for applying upgrades on the execution engine.
use std::{cell::RefCell, fmt, rc::Rc};

use thiserror::Error;

use casper_storage::global_state::state::StateProvider;
use casper_types::{
    bytesrepr::{self},
    execution::ExecutionJournal,
    system::SystemContractType,
    Contract, ContractHash, Digest, Key, ProtocolVersion, StoredValue,
};

use crate::tracking_copy::TrackingCopy;

/// Represents a successfully executed upgrade.
#[derive(Debug, Clone)]
pub struct UpgradeSuccess {
    /// New state root hash generated after effects were applied.
    pub post_state_hash: Digest,
    /// Effects of executing an upgrade request.
    pub effects: ExecutionJournal,
}

impl fmt::Display for UpgradeSuccess {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "Success: {} {:?}", self.post_state_hash, self.effects)
    }
}

/// Represents outcomes of a failed protocol upgrade.
#[derive(Clone, Error, Debug)]
pub enum ProtocolUpgradeError {
    /// Error validating a protocol upgrade config.
    #[error("Invalid upgrade config")]
    InvalidUpgradeConfig,
    /// Unable to retrieve a system contract.
    #[error("Unable to retrieve system contract: {0}")]
    UnableToRetrieveSystemContract(String),
    /// Unable to retrieve a system contract package.
    #[error("Unable to retrieve system contract package: {0}")]
    UnableToRetrieveSystemContractPackage(String),
    /// Unable to disable previous version of a system contract.
    #[error("Failed to disable previous version of system contract: {0}")]
    FailedToDisablePreviousVersion(String),
    /// (De)serialization error.
    #[error("{0}")]
    Bytesrepr(bytesrepr::Error),
    /// Failed to create system contract registry.
    #[error("Failed to insert system contract registry")]
    FailedToCreateSystemRegistry,
    /// Found unexpected variant of a stored value.
    #[error("Unexpected stored value variant")]
    UnexpectedStoredValueVariant,
}

impl From<bytesrepr::Error> for ProtocolUpgradeError {
    fn from(error: bytesrepr::Error) -> Self {
        ProtocolUpgradeError::Bytesrepr(error)
    }
}

/// The system upgrader deals with conducting an actual protocol upgrade.
pub(crate) struct SystemUpgrader<S>
where
    S: StateProvider,
{
    new_protocol_version: ProtocolVersion,
    old_protocol_version: ProtocolVersion,
    tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
}

impl<S> SystemUpgrader<S>
where
    S: StateProvider,
{
    /// Creates new system upgrader instance.
    pub(crate) fn new(
        new_protocol_version: ProtocolVersion,
        old_protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Self {
        SystemUpgrader {
            new_protocol_version,
            old_protocol_version,
            tracking_copy,
        }
    }

    /// Bump major version and/or update the entry points for system contracts.
    pub(crate) fn refresh_system_contracts(
        &self,
        mint_hash: &ContractHash,
        auction_hash: &ContractHash,
        handle_payment_hash: &ContractHash,
        standard_payment_hash: &ContractHash,
    ) -> Result<(), ProtocolUpgradeError> {
        self.refresh_system_contract_entry_points(*mint_hash, SystemContractType::Mint)?;
        self.refresh_system_contract_entry_points(*auction_hash, SystemContractType::Auction)?;
        self.refresh_system_contract_entry_points(
            *handle_payment_hash,
            SystemContractType::HandlePayment,
        )?;
        self.refresh_system_contract_entry_points(
            *standard_payment_hash,
            SystemContractType::StandardPayment,
        )?;

        Ok(())
    }

    /// Refresh the system contracts with an updated set of entry points,
    /// and bump the contract version at a major version upgrade.
    fn refresh_system_contract_entry_points(
        &self,
        contract_hash: ContractHash,
        system_contract_type: SystemContractType,
    ) -> Result<(), ProtocolUpgradeError> {
        let contract_name = system_contract_type.contract_name();
        let entry_points = system_contract_type.contract_entry_points();

        let mut contract = if let StoredValue::Contract(contract) = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::Hash(contract_hash.value()))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(contract_name.to_string())
            })?
            .ok_or_else(|| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(contract_name.to_string())
            })? {
            contract
        } else {
            return Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
                contract_name,
            ));
        };

        let is_major_bump = self
            .old_protocol_version
            .check_next_version(&self.new_protocol_version)
            .is_major_version();

        let entry_points_unchanged = *contract.entry_points() == entry_points;
        if entry_points_unchanged && !is_major_bump {
            // We don't need to do anything if entry points are unchanged, or there's no major
            // version bump.
            return Ok(());
        }

        let contract_package_key = Key::Hash(contract.contract_package_hash().value());

        let mut contract_package = if let StoredValue::ContractPackage(contract_package) = self
            .tracking_copy
            .borrow_mut()
            .read(&contract_package_key)
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                    contract_name.to_string(),
                )
            })?
            .ok_or_else(|| {
                ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                    contract_name.to_string(),
                )
            })? {
            contract_package
        } else {
            return Err(ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                contract_name,
            ));
        };

        contract_package
            .disable_contract_version(contract_hash)
            .map_err(|_| {
                ProtocolUpgradeError::FailedToDisablePreviousVersion(contract_name.to_string())
            })?;
        contract.set_protocol_version(self.new_protocol_version);

        let new_contract = Contract::new(
            contract.contract_package_hash(),
            contract.contract_wasm_hash(),
            contract.named_keys().clone(),
            entry_points,
            self.new_protocol_version,
        );
        self.tracking_copy
            .borrow_mut()
            .write(contract_hash.into(), StoredValue::Contract(new_contract));

        contract_package
            .insert_contract_version(self.new_protocol_version.value().major, contract_hash);

        self.tracking_copy.borrow_mut().write(
            contract_package_key,
            StoredValue::ContractPackage(contract_package),
        );

        Ok(())
    }
}
