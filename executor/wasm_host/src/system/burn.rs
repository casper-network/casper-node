//! Burn.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::mint::Mint, AddressGenerator, RuntimeNativeConfig,
    TrackingCopy,
};
use casper_types::{
    account::AccountHash, system::mint::METHOD_BURN, ApiError, Key, RuntimeFootprint,
    TransactionHash, URef, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct BurnArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    purse: URef,
    amount: U512,
}

impl BurnArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_native_config: RuntimeNativeConfig,
        id: TransactionHash,
        address_generator: Arc<RwLock<AddressGenerator>>,
        initiator: AccountHash,
        context_key: Key,
        remaining_spending_limit: U512,
        purse: URef,
        amount: U512,
    ) -> Self {
        BurnArgs {
            runtime_native_config,
            id,
            address_generator,
            initiator,
            context_key,
            remaining_spending_limit,
            purse,
            amount,
        }
    }
}

pub fn burn<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: BurnArgs,
) -> Result<(), DispatchError> {
    debug!(?args, METHOD_BURN);
    let result = match super::dispatch_userland_to_system_contract(
        tracking_copy,
        runtime_footprint,
        args.runtime_native_config,
        args.id,
        args.address_generator,
        args.initiator,
        args.context_key,
        args.remaining_spending_limit,
        |mut runtime| runtime.burn(args.purse, args.amount),
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "burn failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_BURN);

    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::mint::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, "burn failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
