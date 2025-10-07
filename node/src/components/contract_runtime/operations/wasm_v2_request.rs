use std::sync::Arc;

use super::MetaTransaction;
use bytes::Bytes;
use casper_executor_wasm::{
    install::{
        InstallContractError, InstallContractRequest, InstallContractRequestBuilder,
        InstallContractResult,
    },
    ExecutorV2,
};
use casper_executor_wasm_interface::{
    executor::{
        ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteWithProviderError,
        ExecuteWithProviderResult, ExecutionKind,
    },
    FatalHostError, GasUsage,
};
use casper_storage::{
    global_state::state::{CommitProvider, StateProvider},
    system::runtime_native,
    AddressGeneratorBuilder,
};
use casper_types::{
    bytesrepr::ToBytes, execution::Effects, BlockHash, Digest, Gas, Key, TransactionArgs,
    TransactionEntryPoint, TransactionInvocationTarget, TransactionRuntimeParams,
    TransactionTarget, U512,
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
    Install(InstallContractResult),
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
}

impl WasmV2Request {
    pub(crate) fn new(
        gas_limit: Gas,
        network_name: impl Into<Arc<str>>,
        runtime_native_config: RuntimeNativeConfig,
        state_root_hash: Digest,
        parent_block_hash: BlockHash,
        block_height: u64,
        transaction: &MetaTransaction,
    ) -> Result<Self, InvalidRequest> {
        let transaction_hash = transaction.hash();
        let initiator_addr = transaction.initiator_addr();

        let gas_limit: u64 = gas_limit
            .value()
            .try_into()
            .map_err(|_| InvalidRequest::InvalidGasLimit(gas_limit.value()))?;

        let address_generator = AddressGeneratorBuilder::default()
            .seed_with(transaction_hash.as_ref())
            .build();

        let session_args = transaction.session_args();

        let input_data = match session_args.into_owned() {
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

        let value = transaction
            .transferred_value()
            .ok_or(InvalidRequest::ExpectedTransferredValue)?;

        enum Target {
            Install {
                module_bytes: Bytes,
                entry_point: String,
                transferred_value: u64,
                seed: Option<[u8; 32]>,
            },
            Session {
                module_bytes: Bytes,
            },
            Stored {
                id: TransactionInvocationTarget,
                entry_point: String,
            },
        }

        let transaction_target = transaction.target().ok_or(InvalidRequest::ExpectedTarget)?;
        let target = match transaction_target {
            TransactionTarget::Native => todo!(), //
            TransactionTarget::Stored { id, runtime: _ } => match transaction.entry_point() {
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
                    },
                is_install_upgrade: _, // TODO: Handle this
            } => match transaction.entry_point() {
                TransactionEntryPoint::Call => Target::Session {
                    module_bytes: module_bytes.clone().take_inner().into(),
                },
                TransactionEntryPoint::Custom(entry_point) => Target::Install {
                    module_bytes: module_bytes.clone().take_inner().into(),
                    entry_point: entry_point.to_string(),
                    transferred_value,
                    seed,
                },
                _ => todo!(),
            },
        };

        info!(%transaction_hash, "executing v1 contract");

        match target {
            Target::Install {
                module_bytes,
                entry_point,
                transferred_value,
                seed,
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

                // Value is expected to be the same as transferred value, it's just taken through
                // different API.
                debug_assert_eq!(transferred_value, value);

                let install_request = builder
                    .with_initiator(initiator_addr.account_hash())
                    .with_gas_limit(gas_limit)
                    .with_transaction_hash(transaction_hash)
                    .with_wasm_bytes(module_bytes)
                    .with_address_generator(address_generator)
                    .with_transferred_value(value)
                    .with_chain_name(network_name)
                    .with_block_time(transaction.timestamp().into())
                    .with_state_hash(state_root_hash)
                    .with_parent_block_hash(parent_block_hash)
                    .with_block_height(block_height)
                    .with_runtime_native_config(runtime_native_config)
                    .with_authorization_keys(transaction.signers())
                    .build()
                    .expect("should build");

                Ok(Self::Install(install_request))
            }
            Target::Session { .. } | Target::Stored { .. } => {
                let mut builder = ExecuteRequestBuilder::default();

                let initiator_account_hash = &initiator_addr.account_hash();

                let initiator_key = Key::Account(*initiator_account_hash);

                builder = builder
                    .with_address_generator(address_generator)
                    .with_gas_limit(gas_limit)
                    .with_transaction_hash(transaction_hash)
                    .with_initiator(*initiator_account_hash)
                    .with_caller_key(initiator_key)
                    .with_chain_name(network_name)
                    .with_transferred_value(value)
                    .with_block_time(transaction.timestamp().into())
                    .with_input(input_data)
                    .with_state_hash(state_root_hash)
                    .with_parent_block_hash(parent_block_hash)
                    .with_block_height(block_height)
                    .with_runtime_native_config(runtime_native_config);
                let execution_kind = match target {
                    Target::Session { module_bytes } => ExecutionKind::SessionBytes(module_bytes),
                    Target::Stored {
                        id: TransactionInvocationTarget::ByHash(smart_contract_addr),
                        entry_point,
                    } => ExecutionKind::Stored {
                        address: smart_contract_addr,
                        entry_point: entry_point.clone(),
                    },
                    Target::Stored { id, entry_point } => {
                        todo!("Unsupported target {entry_point} {id:?}")
                    }
                    Target::Install { .. } => unreachable!(),
                };

                builder = builder.with_execution_kind(execution_kind);

                let authorization_keys = transaction.signers();

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
                match engine.install_contract(state_root_hash, state_provider, install_request) {
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
