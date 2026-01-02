use std::{collections::BTreeSet, sync::Arc};

use super::MetaTransaction;
use bytes::Bytes;
use casper_executor_wasm::ExecutorV2;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::{
    executor::{
        ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteWithProviderError,
        ExecuteWithProviderResult, ExecutionKind, PackagePointer,
    },
    install::{
        InstallContractError, InstallContractRequest, InstallContractRequestBuilder,
        InstallContractWithProviderResult,
    },
    FatalHostError, GasUsage,
};
use casper_storage::{
    global_state::state::{CommitProvider, StateProvider},
    system::runtime_native,
    AddressGeneratorBuilder,
};
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, contract_messages::Messages, execution::Effects,
    BlockHash, BlockTime, Digest, Gas, Key, TransactionArgs, TransactionEntryPoint,
    TransactionHash, TransactionInvocationTarget, TransactionRuntimeParams, TransactionTarget,
    U512,
};
use thiserror::Error;
use tracing::info;

use runtime_native::Config as RuntimeNativeConfig;

/// The request to execute a Wasm contract.
pub(crate) enum WasmV2Request {
    /// The request to install a Wasm contract.
    Install(InstallContractRequest),
    /// The request to execute a Wasm contract.
    Execute(ExecuteRequest),
}

/// The result of executing a Wasm contract.
pub(crate) enum WasmV2Result {
    /// The result of installing a Wasm contract.
    Install(InstallContractWithProviderResult),
    /// The result of executing a Wasm contract.
    Execute(ExecuteWithProviderResult),
}

impl WasmV2Result {
    /// Returns the gas usage of the contract execution.
    pub(crate) fn gas_usage(&self) -> &GasUsage {
        match self {
            WasmV2Result::Install(result) => result.gas_usage(),
            WasmV2Result::Execute(result) => result.gas_usage(),
        }
    }

    /// Returns the effects of the contract execution.
    pub(crate) fn effects(&self) -> &Effects {
        match self {
            WasmV2Result::Install(result) => result.effects(),
            WasmV2Result::Execute(result) => result.effects(),
        }
    }

    pub(crate) fn post_state_hash(&self) -> Digest {
        match self {
            WasmV2Result::Install(result) => result.post_state_hash(),
            WasmV2Result::Execute(result) => result.post_state_hash(),
        }
    }

    pub(crate) fn host_error(&self) -> Option<&CallError> {
        match self {
            WasmV2Result::Install(_) => None,
            WasmV2Result::Execute(execute_with_provider_result) => {
                execute_with_provider_result.host_error.as_ref()
            }
        }
    }

    pub(crate) fn messages(&self) -> &Messages {
        match self {
            WasmV2Result::Install(install_contract_result) => install_contract_result.messages(),
            WasmV2Result::Execute(execute_with_provider_result) => {
                execute_with_provider_result.messages()
            }
        }
    }

    pub(crate) fn output(&self) -> Option<&Bytes> {
        match self {
            WasmV2Result::Install(_) => None,
            WasmV2Result::Execute(execute_with_provider_result) => {
                execute_with_provider_result.output()
            }
        }
    }
}

#[derive(Error, Debug)]
pub(crate) enum WasmV2Error {
    #[error(transparent)]
    Install(InstallContractError),
    #[error(transparent)]
    Execute(ExecuteWithProviderError),
}

impl WasmV2Error {
    pub(crate) fn as_internal_host_error(&self) -> Option<FatalHostError> {
        match self {
            WasmV2Error::Install(install_error) => {
                if let InstallContractError::Execute(ExecuteError::Fatal(internal_host_error)) =
                    install_error
                {
                    return Some(internal_host_error.clone());
                }
                None
            }
            WasmV2Error::Execute(execute_with_provider_error) => {
                if let ExecuteWithProviderError::Execute(ExecuteError::Fatal(internal_host_error)) =
                    execute_with_provider_error
                {
                    let err = internal_host_error.clone();
                    return Some(err);
                }
                None
            }
        }
    }

    pub(crate) fn gas_usage(&self) -> Option<&GasUsage> {
        match self {
            WasmV2Error::Install(install_error) => install_error.gas_usage(),
            WasmV2Error::Execute(_) => None,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Error, Debug)]
pub(crate) enum InvalidRequest {
    #[error("Expected target")]
    ExpectedTarget,
    #[error("Invalid gas limit: {0}")]
    InvalidGasLimit(U512),
    #[error("Expected transferred value")]
    ExpectedTransferredValue,
    #[error("Expected V2 runtime")]
    ExpectedV2Runtime,
    #[error("Invalid input")]
    InvalidaInput,
}

impl WasmV2Request {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        gas_limit: Gas,
        network_name: impl Into<Arc<str>>,
        runtime_native_config: RuntimeNativeConfig,
        state_root_hash: Digest,
        parent_block_hash: BlockHash,
        block_height: u64,
        block_time: BlockTime,
        transaction: &MetaTransaction,
    ) -> Result<Self, InvalidRequest> {
        let transaction_hash = transaction.hash();
        let initiator_addr = transaction.initiator_addr();
        let value = transaction
            .transferred_value()
            .ok_or(InvalidRequest::ExpectedTransferredValue)?;
        let transaction_target = transaction.target().ok_or(InvalidRequest::ExpectedTarget)?;
        let entry_point = transaction.entry_point();
        let signers = transaction.signers();
        let session_args = transaction.session_args().into_owned();
        Self::new_with_args(
            gas_limit,
            network_name,
            runtime_native_config,
            state_root_hash,
            parent_block_hash,
            block_height,
            transaction_hash,
            initiator_addr.account_hash(),
            session_args,
            value,
            transaction_target,
            entry_point,
            block_time,
            signers,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_args(
        gas_limit: Gas,
        network_name: impl Into<Arc<str>>,
        runtime_native_config: RuntimeNativeConfig,
        state_root_hash: Digest,
        parent_block_hash: BlockHash,
        block_height: u64,
        transaction_hash: TransactionHash,
        initiator_addr: AccountHash,
        session_args: TransactionArgs,
        transferred_value: u64,
        transaction_target: TransactionTarget,
        entry_point: TransactionEntryPoint,
        block_time: BlockTime,
        signers: BTreeSet<AccountHash>,
        sandboxed: bool,
    ) -> Result<Self, InvalidRequest> {
        let gas_limit: u64 = gas_limit
            .value()
            .try_into()
            .map_err(|_| InvalidRequest::InvalidGasLimit(gas_limit.value()))?;

        let address_generator = AddressGeneratorBuilder::default()
            .seed_with(transaction_hash.as_ref())
            .build();

        let input_data = match session_args {
            TransactionArgs::Named(named_args) => {
                // Named arguments are expected to be in the form of a map.
                // This is the case for VmCasperV1 runtime.
                named_args
                    .to_bytes()
                    .map(Bytes::from)
                    .map_err(|_| InvalidRequest::ExpectedTarget)?
            }

            TransactionArgs::Bytesrepr(bytes) => bytes.take_inner().into(),
        };

        enum Target {
            Install {
                module_bytes: Bytes,
                entry_point: String,
                transferred_value: u64,
                seed: Option<[u8; 32]>,
                bundle_data: Option<Bytes>,
            },
            Session {
                module_bytes: Bytes,
            },
            Stored {
                id: TransactionInvocationTarget,
                entry_point: String,
            },
        }

        let target = match transaction_target {
            TransactionTarget::Native => todo!(), //
            TransactionTarget::Stored { id, runtime: _ } => match entry_point {
                TransactionEntryPoint::Custom(entry_point) => Target::Stored {
                    id: id.clone(),
                    entry_point: entry_point.clone(),
                },
                _ => todo!(),
            },

            TransactionTarget::Session {
                module_bytes: _,
                runtime: TransactionRuntimeParams::VmCasperV1,
                is_install_upgrade: _, // TODO: Handle this
            } => {
                return Err(InvalidRequest::ExpectedV2Runtime);
            }
            TransactionTarget::Session {
                module_bytes,
                runtime:
                    TransactionRuntimeParams::VmCasperV2 {
                        transferred_value,
                        seed,
                        bundle_data,
                    },
                is_install_upgrade,
            } => {
                if is_install_upgrade {
                    let entry_point_name = match entry_point {
                        TransactionEntryPoint::Custom(entry_point) => entry_point,
                        // For vm2 session install/upgrade we expect a constructor name
                        // to be specified verbatim in the TransactionEntryPoint::Custom variant
                        _ => return Err(InvalidRequest::InvalidaInput),
                    };
                    Target::Install {
                        module_bytes: module_bytes.clone().take_inner().into(),
                        entry_point: entry_point_name,
                        transferred_value,
                        seed,
                        bundle_data: bundle_data.map(|bytes| bytes.take_inner().into()),
                    }
                } else {
                    Target::Session {
                        module_bytes: module_bytes.clone().take_inner().into(),
                    }
                }
            }
        };

        info!(%transaction_hash, "executing v1 contract");

        match target {
            Target::Install {
                module_bytes,
                entry_point,
                transferred_value,
                seed,
                bundle_data,
            } => {
                let mut builder = InstallContractRequestBuilder::default();

                let entry_point = (!entry_point.is_empty()).then_some(entry_point);

                match entry_point {
                    Some(entry_point) => {
                        builder = builder
                            .with_entry_point(entry_point.clone())
                            // Args only matter if there is a constructor to be called.
                            .with_input(input_data.clone());
                    }
                    None => {
                        // No input data expected if there is no entry point. This should be
                        // validated in transaction acceptor.
                        assert!(input_data.is_empty());
                    }
                }

                if let Some(seed) = seed {
                    builder = builder.with_seed(seed);
                }

                if let Some(bundle_data) = bundle_data {
                    builder = builder.with_bundle_data(bundle_data);
                }

                let install_request = builder
                    .with_initiator(initiator_addr)
                    .with_gas_limit(gas_limit)
                    .with_transaction_hash(transaction_hash)
                    .with_wasm_bytes(module_bytes)
                    .with_address_generator(address_generator)
                    .with_transferred_value(transferred_value)
                    .with_chain_name(network_name)
                    .with_block_time(block_time)
                    .with_state_hash(state_root_hash)
                    .with_parent_block_hash(parent_block_hash)
                    .with_block_height(block_height)
                    .with_runtime_native_config(runtime_native_config)
                    .with_authorization_keys(signers)
                    .with_sandboxed(sandboxed)
                    .build()
                    .expect("should build");

                Ok(Self::Install(install_request))
            }
            Target::Session { .. } | Target::Stored { .. } => {
                let mut builder = ExecuteRequestBuilder::default();

                let initiator_account_hash = &initiator_addr;

                let initiator_key = Key::Account(*initiator_account_hash);

                builder = builder
                    .with_address_generator(address_generator)
                    .with_gas_limit(gas_limit)
                    .with_transaction_hash(transaction_hash)
                    .with_initiator(*initiator_account_hash)
                    .with_caller_key(initiator_key)
                    .with_chain_name(network_name)
                    .with_transferred_value(transferred_value)
                    .with_block_time(block_time)
                    .with_input(input_data)
                    .with_state_hash(state_root_hash)
                    .with_parent_block_hash(parent_block_hash)
                    .with_block_height(block_height)
                    .with_runtime_native_config(runtime_native_config)
                    .with_sandboxed(sandboxed);
                let execution_kind = match target {
                    Target::Session { module_bytes } => ExecutionKind::SessionBytes(module_bytes),
                    Target::Stored {
                        id: TransactionInvocationTarget::ByHash(smart_contract_addr),
                        entry_point,
                    } => ExecutionKind::Stored {
                        package_pointer: PackagePointer::HashAddr(smart_contract_addr),
                        entry_point: entry_point.clone(),
                        version: None,
                        protocol_version_major: None,
                    },
                    Target::Stored {
                        id:
                            TransactionInvocationTarget::ByPackageHash {
                                addr,
                                version,
                                protocol_version_major,
                            },
                        entry_point,
                    } => ExecutionKind::Stored {
                        package_pointer: PackagePointer::HashAddr(addr.value()),
                        entry_point: entry_point.clone(),
                        version,
                        protocol_version_major,
                    },
                    Target::Stored {
                        id:
                            TransactionInvocationTarget::ByPackageName {
                                name,
                                version,
                                protocol_version_major,
                            },
                        entry_point,
                    } => ExecutionKind::Stored {
                        package_pointer: PackagePointer::NamedKeyName(name),
                        entry_point: entry_point.clone(),
                        version,
                        protocol_version_major,
                    },
                    Target::Stored { id, entry_point } => {
                        todo!("Unsupported target {entry_point} {id:?}")
                    }
                    Target::Install { .. } => unreachable!(),
                };

                builder = builder.with_execution_kind(execution_kind);

                let authorization_keys = signers;

                let execute_request = builder
                    .with_authorization_keys(authorization_keys)
                    .build()
                    .expect("should build");

                Ok(Self::Execute(execute_request))
            }
        }
    }

    pub(crate) fn execute<P>(
        self,
        engine: &ExecutorV2,
        state_root_hash: Digest,
        state_provider: &P,
    ) -> Result<WasmV2Result, WasmV2Error>
    where
        P: StateProvider + CommitProvider,
        <P as StateProvider>::Reader: 'static,
    {
        match self {
            WasmV2Request::Install(install_request) => {
                match engine.install_contract_with_provider(
                    state_root_hash,
                    state_provider,
                    install_request,
                ) {
                    Ok(result) => Ok(WasmV2Result::Install(result)),
                    Err(error) => Err(WasmV2Error::Install(error)),
                }
            }
            WasmV2Request::Execute(execute_request) => {
                match engine.execute_with_provider(state_root_hash, state_provider, execute_request)
                {
                    Ok(result) => Ok(WasmV2Result::Execute(result)),
                    Err(error) => Err(WasmV2Error::Execute(error)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_test() {}
}
