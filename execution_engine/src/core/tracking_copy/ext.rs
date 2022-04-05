use std::convert::TryInto;

use casper_types::{
    account::{Account, AccountHash},
    CLValue, Contract, ContractHash, ContractPackage, ContractPackageHash, ContractWasm,
    ContractWasmHash, Key, Motes, StoredValue, StoredValueTypeMismatch, URef,
};

use crate::{
    core::{engine_state::SystemContractRegistry, execution, tracking_copy::TrackingCopy},
    shared::newtypes::CorrelationId,
    storage::{global_state::StateReader, trie::merkle_proof::TrieMerkleProof},
};

pub trait TrackingCopyExt<R> {
    type Error;

    /// Gets the account at a given account address
    fn get_account(
        &mut self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
    ) -> Result<Account, Self::Error>;

    /// Reads the account at a given account address
    fn read_account(
        &mut self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
    ) -> Result<Account, Self::Error>;

    // TODO: make this a static method
    /// Gets the purse balance key for a given purse id
    fn get_purse_balance_key(
        &self,
        correlation_id: CorrelationId,
        purse_key: Key,
    ) -> Result<Key, Self::Error>;

    /// Gets the balance at a given balance key
    fn get_purse_balance(
        &self,
        correlation_id: CorrelationId,
        balance_key: Key,
    ) -> Result<Motes, Self::Error>;

    /// Gets the purse balance key for a given purse id and provides a Merkle proof
    fn get_purse_balance_key_with_proof(
        &self,
        correlation_id: CorrelationId,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets the balance at a given balance key and provides a Merkle proof
    fn get_purse_balance_with_proof(
        &self,
        correlation_id: CorrelationId,
        balance_key: Key,
    ) -> Result<(Motes, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets a contract by Key
    fn get_contract_wasm(
        &mut self,
        correlation_id: CorrelationId,
        contract_wasm_hash: ContractWasmHash,
    ) -> Result<ContractWasm, Self::Error>;

    /// Gets a contract header by Key
    fn get_contract(
        &mut self,
        correlation_id: CorrelationId,
        contract_hash: ContractHash,
    ) -> Result<Contract, Self::Error>;

    /// Gets a contract package by Key
    fn get_contract_package(
        &mut self,
        correlation_id: CorrelationId,
        contract_package_hash: ContractPackageHash,
    ) -> Result<ContractPackage, Self::Error>;

    fn get_system_contracts(
        &mut self,
        correlation_id: CorrelationId,
    ) -> Result<SystemContractRegistry, Self::Error>;
}

impl<R> TrackingCopyExt<R> for TrackingCopy<R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = execution::Error;

    fn get_account(
        &mut self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
    ) -> Result<Account, Self::Error> {
        let account_key = Key::Account(account_hash);
        match self.get(correlation_id, &account_key).map_err(Into::into)? {
            Some(StoredValue::Account(account)) => Ok(account),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("Account".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(account_key)),
        }
    }

    fn read_account(
        &mut self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
    ) -> Result<Account, Self::Error> {
        let account_key = Key::Account(account_hash);
        match self
            .read(correlation_id, &account_key)
            .map_err(Into::into)?
        {
            Some(StoredValue::Account(account)) => Ok(account),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("Account".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(account_key)),
        }
    }

    fn get_purse_balance_key(
        &self,
        _correlation_id: CorrelationId,
        purse_key: Key,
    ) -> Result<Key, Self::Error> {
        let balance_key: URef = purse_key
            .into_uref()
            .ok_or(execution::Error::KeyIsNotAURef(purse_key))?;
        Ok(Key::Balance(balance_key.addr()))
    }

    fn get_purse_balance(
        &self,
        correlation_id: CorrelationId,
        key: Key,
    ) -> Result<Motes, Self::Error> {
        let stored_value: StoredValue = self
            .read(correlation_id, &key)
            .map_err(Into::into)?
            .ok_or(execution::Error::KeyNotFound(key))?;
        let cl_value: CLValue = stored_value
            .try_into()
            .map_err(execution::Error::TypeMismatch)?;
        let balance = Motes::new(cl_value.into_t()?);
        Ok(balance)
    }

    fn get_purse_balance_key_with_proof(
        &self,
        correlation_id: CorrelationId,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let balance_key: Key = purse_key
            .uref_to_hash()
            .ok_or(execution::Error::KeyIsNotAURef(purse_key))?;
        let proof: TrieMerkleProof<Key, StoredValue> = self
            .read_with_proof(correlation_id, &balance_key) // Key::Hash, so no need to normalize
            .map_err(Into::into)?
            .ok_or(execution::Error::KeyNotFound(purse_key))?;
        let stored_value_ref: &StoredValue = proof.value();
        let cl_value: CLValue = stored_value_ref
            .to_owned()
            .try_into()
            .map_err(execution::Error::TypeMismatch)?;
        let balance_key: Key = cl_value.into_t()?;
        Ok((balance_key, proof))
    }

    fn get_purse_balance_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: Key,
    ) -> Result<(Motes, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let proof: TrieMerkleProof<Key, StoredValue> = self
            .read_with_proof(correlation_id, &key.normalize())
            .map_err(Into::into)?
            .ok_or(execution::Error::KeyNotFound(key))?;
        let cl_value: CLValue = proof
            .value()
            .to_owned()
            .try_into()
            .map_err(execution::Error::TypeMismatch)?;
        let balance = Motes::new(cl_value.into_t()?);
        Ok((balance, proof))
    }

    /// Gets a contract wasm by Key
    fn get_contract_wasm(
        &mut self,
        correlation_id: CorrelationId,
        contract_wasm_hash: ContractWasmHash,
    ) -> Result<ContractWasm, Self::Error> {
        let key = contract_wasm_hash.into();
        match self.get(correlation_id, &key).map_err(Into::into)? {
            Some(StoredValue::ContractWasm(contract_wasm)) => Ok(contract_wasm),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("ContractWasm".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(key)),
        }
    }

    /// Gets a contract header by Key
    fn get_contract(
        &mut self,
        correlation_id: CorrelationId,
        contract_hash: ContractHash,
    ) -> Result<Contract, Self::Error> {
        let key = contract_hash.into();
        match self.read(correlation_id, &key).map_err(Into::into)? {
            Some(StoredValue::Contract(contract)) => Ok(contract),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("Contract".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(key)),
        }
    }

    fn get_contract_package(
        &mut self,
        correlation_id: CorrelationId,
        contract_package_hash: ContractPackageHash,
    ) -> Result<ContractPackage, Self::Error> {
        let key = contract_package_hash.into();
        match self.read(correlation_id, &key).map_err(Into::into)? {
            Some(StoredValue::ContractPackage(contract_package)) => Ok(contract_package),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("ContractPackage".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(key)),
        }
    }

    fn get_system_contracts(
        &mut self,
        correlation_id: CorrelationId,
    ) -> Result<SystemContractRegistry, Self::Error> {
        match self
            .get(correlation_id, &Key::SystemContractRegistry)
            .map_err(Into::into)?
        {
            Some(StoredValue::CLValue(registry)) => {
                let registry: SystemContractRegistry =
                    CLValue::into_t(registry).map_err(Self::Error::from)?;
                Ok(registry)
            }
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(Key::SystemContractRegistry)),
        }
    }
}
