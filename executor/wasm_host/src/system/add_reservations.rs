//! Add Reservations.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::{Reservation, METHOD_ADD_RESERVATIONS},
    ApiError, TransactionHash,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct AddReservationsArgs {
    reservations: Vec<Reservation>,
}

impl AddReservationsArgs {
    pub fn new(reservations: Vec<Reservation>) -> Self {
        AddReservationsArgs { reservations }
    }
}

pub fn add_reservations<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: AddReservationsArgs,
) -> Result<(), DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| runtime.add_reservations(args.reservations.clone()),
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "add reservations failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_ADD_RESERVATIONS);

    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, ?args, "add reservations failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
