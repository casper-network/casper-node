//! Cancel Reservations.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::{DelegatorKind, METHOD_CANCEL_RESERVATIONS},
    ApiError, PublicKey, TransactionHash,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct CancelReservationsArgs {
    validator: PublicKey,
    delegators: Vec<DelegatorKind>,
    max_delegators_per_validator: u32,
}

impl CancelReservationsArgs {
    pub fn new(
        validator: PublicKey,
        delegators: Vec<DelegatorKind>,
        max_delegators_per_validator: u32,
    ) -> Self {
        CancelReservationsArgs {
            validator,
            delegators,
            max_delegators_per_validator,
        }
    }
}

pub fn cancel_reservations<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: CancelReservationsArgs,
) -> Result<(), DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.cancel_reservations(
                args.validator.clone(),
                args.delegators.clone(),
                args.max_delegators_per_validator,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "cancel reservation failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_CANCEL_RESERVATIONS);

    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, ?args, "cancel reservation failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
