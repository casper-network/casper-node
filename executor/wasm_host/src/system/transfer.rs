//! Transfer.

use std::sync::Arc;

use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::mint::Mint, AddressGenerator, RuntimeNativeConfig,
    TrackingCopy,
};
use casper_types::{
    account::AccountHash, ApiError, Key, RuntimeFootprint, TransactionHash, URef, METHOD_TRANSFER,
    U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

use crate::system::DispatchError;

#[derive(Debug, Clone)]
pub struct TransferArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    maybe_to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    transfer_id: Option<u64>,
}

impl TransferArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_native_config: RuntimeNativeConfig,
        id: TransactionHash,
        address_generator: Arc<RwLock<AddressGenerator>>,
        initiator: AccountHash,
        context_key: Key,
        remaining_spending_limit: U512,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Self {
        TransferArgs {
            runtime_native_config,
            id,
            address_generator,
            initiator,
            context_key,
            remaining_spending_limit,
            source,
            target,
            amount,
            maybe_to: None,
            transfer_id: None,
        }
    }
}

pub fn transfer<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: TransferArgs,
) -> Result<(), DispatchError> {
    debug!(?args, METHOD_TRANSFER);
    let transfer_result = match super::dispatch_userland_to_system_contract(
        tracking_copy,
        runtime_footprint,
        args.runtime_native_config,
        args.id,
        args.address_generator,
        args.initiator,
        args.context_key,
        args.remaining_spending_limit,
        |mut runtime| {
            runtime.transfer(
                args.maybe_to,
                args.source,
                args.target,
                args.amount,
                args.transfer_id,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "transfer failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?transfer_result, METHOD_TRANSFER);

    match transfer_result {
        Ok(()) => Ok(()),
        Err(casper_types::system::mint::Error::InsufficientFunds) => {
            Err(DispatchError::Call(CallError::CalleeRolledBack))
        }
        Err(casper_types::system::mint::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, "transfer failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
