//! Transfer.

use std::sync::Arc;

use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::mint::Mint, AddressGenerator, RuntimeNativeConfig,
    TrackingCopy,
};
use casper_types::{account::AccountHash, ApiError, TransactionHash, URef, METHOD_TRANSFER, U512};
use parking_lot::RwLock;
use tracing::{debug, error};

use crate::system::{dispatch_system_contract, DispatchError};

#[derive(Debug, Copy, Clone)]
pub struct TransferArgs {
    maybe_to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
}

impl TransferArgs {
    pub fn new(source: URef, target: URef, amount: U512) -> Self {
        TransferArgs {
            source,
            target,
            amount,
            maybe_to: None,
            id: None,
        }
    }
}

pub fn transfer<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: TransferArgs,
) -> Result<(), DispatchError> {
    let transfer_result: Result<(), casper_types::system::mint::Error> =
        match dispatch_system_contract(
            tracking_copy,
            runtime_native_config,
            id,
            address_generator,
            |mut runtime| {
                let TransferArgs {
                    maybe_to,
                    source,
                    target,
                    amount,
                    id,
                } = args;

                runtime.transfer(maybe_to, source, target, amount, id)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                error!(%error, "transfer failed on dispatch");
                return Err(DispatchError::Internal(
                    InternalHostError::DispatchSystemContract,
                ));
            }
        };

    debug!(?args, ?transfer_result, METHOD_TRANSFER);

    match transfer_result {
        Ok(()) => Ok(()),
        Err(casper_types::system::mint::Error::InsufficientFunds) => {
            Err(DispatchError::Call(CallError::CalleeReverted))
        }
        Err(casper_types::system::mint::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, ?args, "transfer failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
