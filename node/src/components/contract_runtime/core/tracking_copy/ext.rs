use std::convert::TryInto;

use parity_wasm::elements::Module;

use types::{
    account::AccountHash, CLValue, Contract, ContractHash, ContractPackage, ContractPackageHash,
    ContractWasm, ContractWasmHash, Key, U512,
};

use crate::{
    components::contract_runtime::{
        core::{execution, tracking_copy::TrackingCopy},
        shared::{
            account::Account,
            newtypes::CorrelationId,
            stored_value::StoredValue,
            wasm,
            wasm_prep::{self, Preprocessor},
            TypeMismatch,
        },
        storage::global_state::StateReader,
    },
    types::Motes,
};

pub trait TrackingCopyExt<R> {
    type Error;

    /// Gets the account at a given account address.
    fn get_account(
        &mut self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
    ) -> Result<Account, Self::Error>;

    /// Reads the account at a given account address.
    fn read_account(
        &mut self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
    ) -> Result<Account, Self::Error>;

    /// Gets the purse balance key for a given purse id
    fn get_purse_balance_key(
        &mut self,
        correlation_id: CorrelationId,
        purse_key: Key,
    ) -> Result<Key, Self::Error>;

    /// Gets the balance at a given balance key
    fn get_purse_balance(
        &mut self,
        correlation_id: CorrelationId,
        balance_key: Key,
    ) -> Result<Motes, Self::Error>;

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

    fn get_system_module(
        &mut self,
        correlation_id: CorrelationId,
        contract_wasm_hash: ContractWasmHash,
        use_system_contracts: bool,
        preprocessor: &Preprocessor,
    ) -> Result<Module, Self::Error>;
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
            Some(other) => Err(execution::Error::TypeMismatch(TypeMismatch::new(
                "Account".to_string(),
                other.type_name(),
            ))),
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
            Some(other) => Err(execution::Error::TypeMismatch(TypeMismatch::new(
                "Account".to_string(),
                other.type_name(),
            ))),
            None => Err(execution::Error::KeyNotFound(account_key)),
        }
    }

    fn get_purse_balance_key(
        &mut self,
        correlation_id: CorrelationId,
        purse_key: Key,
    ) -> Result<Key, Self::Error> {
        let uref = purse_key
            .as_uref()
            .ok_or_else(|| execution::Error::URefNotFound("public purse balance 1".to_string()))?;
        let local_key_bytes = uref.addr();
        let balance_mapping_key = Key::Hash(local_key_bytes);
        match self
            .read(correlation_id, &balance_mapping_key)
            .map_err(Into::into)?
        {
            Some(stored_value) => {
                let cl_value: CLValue = stored_value
                    .try_into()
                    .map_err(execution::Error::TypeMismatch)?;
                Ok(cl_value.into_t()?)
            }
            None => Err(execution::Error::URefNotFound(
                "public purse balance 21".to_string(),
            )),
        }
    }

    fn get_purse_balance(
        &mut self,
        correlation_id: CorrelationId,
        key: Key,
    ) -> Result<Motes, Self::Error> {
        let read_result = match self.read(correlation_id, &key) {
            Ok(read_result) => read_result,
            Err(_) => return Err(execution::Error::KeyNotFound(key)),
        };
        match read_result {
            Some(stored_value) => {
                let cl_value: CLValue = stored_value
                    .try_into()
                    .map_err(execution::Error::TypeMismatch)?;
                let balance: U512 = cl_value.into_t()?;
                Ok(Motes::new(balance))
            }
            None => Err(execution::Error::KeyNotFound(key)),
        }
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
            Some(other) => Err(execution::Error::TypeMismatch(TypeMismatch::new(
                "ContractHeader".to_string(),
                other.type_name(),
            ))),
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
        match self.get(correlation_id, &key).map_err(Into::into)? {
            Some(StoredValue::Contract(contract)) => Ok(contract),
            Some(other) => Err(execution::Error::TypeMismatch(TypeMismatch::new(
                "ContractHeader".to_string(),
                other.type_name(),
            ))),
            None => Err(execution::Error::KeyNotFound(key)),
        }
    }

    fn get_contract_package(
        &mut self,
        correlation_id: CorrelationId,
        contract_package_hash: ContractPackageHash,
    ) -> Result<ContractPackage, Self::Error> {
        let key = contract_package_hash.into();
        match self.get(correlation_id, &key).map_err(Into::into)? {
            Some(StoredValue::ContractPackage(contract_package)) => Ok(contract_package),
            Some(other) => Err(execution::Error::TypeMismatch(TypeMismatch::new(
                "ContractPackage".to_string(),
                other.type_name(),
            ))),
            None => Err(execution::Error::KeyNotFound(key)),
        }
    }

    fn get_system_module(
        &mut self,
        correlation_id: CorrelationId,
        contract_wasm_hash: ContractWasmHash,
        use_system_contracts: bool,
        preprocessor: &Preprocessor,
    ) -> Result<Module, Self::Error> {
        match {
            if use_system_contracts {
                let contract_wasm = match self.get_contract_wasm(correlation_id, contract_wasm_hash)
                {
                    Ok(contract_wasm) => contract_wasm,
                    Err(error) => {
                        return Err(error);
                    }
                };

                wasm_prep::deserialize(contract_wasm.bytes())
            } else {
                wasm::do_nothing_module(preprocessor)
            }
        } {
            Ok(module) => Ok(module),
            Err(error) => Err(error.into()),
        }
    }
}
