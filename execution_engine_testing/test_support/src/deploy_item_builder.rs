use std::{collections::BTreeSet, path::Path};

use rand::Rng;

use casper_execution_engine::core::engine_state::{
    deploy_item::DeployItem, executable_deploy_item::ExecutableDeployItem,
};
use casper_hashing::Digest;
use casper_types::{
    account::AccountHash, ContractHash, ContractPackageHash, ContractVersion, DeployHash, HashAddr,
    RuntimeArgs,
};

use crate::{utils, DEFAULT_GAS_PRICE};

#[derive(Default)]
struct DeployItemData {
    pub address: Option<AccountHash>,
    pub payment_code: Option<ExecutableDeployItem>,
    pub session_code: Option<ExecutableDeployItem>,
    pub gas_price: u64,
    pub authorization_keys: BTreeSet<AccountHash>,
    pub deploy_hash: Option<DeployHash>,
}

/// Builds a [`DeployItem`].
pub struct DeployItemBuilder {
    deploy_item: DeployItemData,
}

impl DeployItemBuilder {
    /// Returns a new [`DeployItemBuilder`] struct.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the address of the deploy.
    pub fn with_address(mut self, address: AccountHash) -> Self {
        self.deploy_item.address = Some(address);
        self
    }

    /// Sets the payment bytes for the deploy.
    pub fn with_payment_bytes(mut self, module_bytes: Vec<u8>, args: RuntimeArgs) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::ModuleBytes {
            module_bytes: module_bytes.into(),
            args,
        });
        self
    }

    /// Sets the payment bytes of the deploy to an empty Vec.
    pub fn with_empty_payment_bytes(self, args: RuntimeArgs) -> Self {
        self.with_payment_bytes(vec![], args)
    }

    /// Sets the payment bytes of a deploy by reading a file and passing [`RuntimeArgs`].
    pub fn with_payment_code<T: AsRef<Path>>(self, file_name: T, args: RuntimeArgs) -> Self {
        let module_bytes = utils::read_wasm_file_bytes(file_name);
        self.with_payment_bytes(module_bytes, args)
    }

    /// Sets payment code of the deploy with contract hash.
    pub fn with_stored_payment_hash(
        mut self,
        hash: ContractHash,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::StoredContractByHash {
            hash,
            entry_point: entry_point.into(),
            args,
        });
        self
    }

    /// Sets the payment code of the deploy with a named key.
    pub fn with_stored_payment_named_key(
        mut self,
        uref_name: &str,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::StoredContractByName {
            name: uref_name.to_owned(),
            entry_point: entry_point_name.into(),
            args,
        });
        self
    }

    /// Sets the payment code of the deploy with a contract package hash.
    pub fn with_stored_versioned_payment_hash(
        mut self,
        package_hash: ContractPackageHash,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::StoredVersionedContractByHash {
            hash: package_hash,
            version: None,
            entry_point: entry_point.into(),
            args,
        });
        self
    }

    /// Sets the payment code of the deploy with versioned contract stored under a named key.
    pub fn with_stored_versioned_payment_named_key(
        mut self,
        uref_name: &str,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::StoredVersionedContractByName {
            name: uref_name.to_owned(),
            version: None,
            entry_point: entry_point_name.into(),
            args,
        });
        self
    }

    /// Sets the session bytes for the deploy.
    pub fn with_session_bytes(mut self, module_bytes: Vec<u8>, args: RuntimeArgs) -> Self {
        self.deploy_item.session_code = Some(ExecutableDeployItem::ModuleBytes {
            module_bytes: module_bytes.into(),
            args,
        });
        self
    }

    /// Sets the session code for the deploy using a wasm file.
    pub fn with_session_code<T: AsRef<Path>>(self, file_name: T, args: RuntimeArgs) -> Self {
        let module_bytes = utils::read_wasm_file_bytes(file_name);
        self.with_session_bytes(module_bytes, args)
    }

    /// Sets the session code of the deploy as a native transfer.
    pub fn with_transfer_args(mut self, args: RuntimeArgs) -> Self {
        self.deploy_item.session_code = Some(ExecutableDeployItem::Transfer { args });
        self
    }

    /// Sets the session code for the deploy with a stored contract hash, entrypoint and runtime
    /// arguments.
    pub fn with_stored_session_hash(
        mut self,
        hash: ContractHash,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.session_code = Some(ExecutableDeployItem::StoredContractByHash {
            hash,
            entry_point: entry_point.into(),
            args,
        });
        self
    }

    /// Sets the session code of the deploy by using a contract stored under a named key.
    pub fn with_stored_session_named_key(
        mut self,
        name: &str,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.session_code = Some(ExecutableDeployItem::StoredContractByName {
            name: name.to_owned(),
            entry_point: entry_point.into(),
            args,
        });
        self
    }

    /// Sets the session code of the deploy with a versioned contract stored under a named key.
    pub fn with_stored_versioned_contract_by_name(
        mut self,
        name: &str,
        version: Option<ContractVersion>,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.session_code = Some(ExecutableDeployItem::StoredVersionedContractByName {
            name: name.to_owned(),
            version,
            entry_point: entry_point.to_owned(),
            args,
        });
        self
    }

    /// Sets the session code of the deploy with a stored, versioned contract by contract hash.
    pub fn with_stored_versioned_contract_by_hash(
        mut self,
        hash: HashAddr,
        version: Option<ContractVersion>,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.session_code = Some(ExecutableDeployItem::StoredVersionedContractByHash {
            hash: hash.into(),
            version,
            entry_point: entry_point.to_owned(),
            args,
        });
        self
    }

    /// Sets the payment code of the deploy with a versioned contract stored under a named key.
    pub fn with_stored_versioned_payment_contract_by_name(
        mut self,
        key_name: &str,
        version: Option<ContractVersion>,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::StoredVersionedContractByName {
            name: key_name.to_owned(),
            version,
            entry_point: entry_point.to_owned(),
            args,
        });
        self
    }

    /// Sets the payment code of the deploy using a stored versioned contract by contract hash.
    pub fn with_stored_versioned_payment_contract_by_hash(
        mut self,
        hash: HashAddr,
        version: Option<ContractVersion>,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        self.deploy_item.payment_code = Some(ExecutableDeployItem::StoredVersionedContractByHash {
            hash: hash.into(),
            version,
            entry_point: entry_point.to_owned(),
            args,
        });
        self
    }

    /// Sets authorization keys for the deploy.
    pub fn with_authorization_keys(mut self, authorization_keys: &[AccountHash]) -> Self {
        self.deploy_item.authorization_keys = authorization_keys.iter().copied().collect();
        self
    }

    /// Sets the gas price for the deploy.
    pub fn with_gas_price(mut self, gas_price: u64) -> Self {
        self.deploy_item.gas_price = gas_price;
        self
    }

    /// Sets the hash of the deploy.
    pub fn with_deploy_hash(mut self, hash: [u8; 32]) -> Self {
        let digest: Digest = hash.into();
        self.deploy_item.deploy_hash = Some(DeployHash::new(digest.value()));
        self
    }

    /// Consumes self and returns a [`DeployItem`].
    pub fn build(self) -> DeployItem {
        DeployItem {
            address: self
                .deploy_item
                .address
                .unwrap_or_else(|| AccountHash::new([0u8; 32])),
            session: self
                .deploy_item
                .session_code
                .expect("should have session code"),
            payment: self
                .deploy_item
                .payment_code
                .expect("should have payment code"),
            gas_price: self.deploy_item.gas_price,
            authorization_keys: self.deploy_item.authorization_keys,
            deploy_hash: self
                .deploy_item
                .deploy_hash
                .unwrap_or_else(|| rand::thread_rng().gen()),
        }
    }
}

impl Default for DeployItemBuilder {
    fn default() -> Self {
        let deploy_item = DeployItemData {
            gas_price: DEFAULT_GAS_PRICE,
            ..Default::default()
        };
        DeployItemBuilder { deploy_item }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_default_deploy_hash_to_zeros_if_not_specified() {
        let address = AccountHash::new([42; 32]);
        let deploy = DeployItemBuilder::new()
            .with_address(address)
            .with_authorization_keys(&[address])
            .with_session_bytes(Vec::new(), RuntimeArgs::new())
            .with_payment_bytes(Vec::new(), RuntimeArgs::new())
            .build();
        assert_ne!(deploy.deploy_hash, DeployHash::default());
    }
}
