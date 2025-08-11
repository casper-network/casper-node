//! Activate Bid.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{system::auction::METHOD_ACTIVATE_BID, PublicKey, TransactionHash};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct ActivateBidArgs {
    validator: PublicKey,
    minimum_bid: u64,
}

impl ActivateBidArgs {
    pub fn new(validator: PublicKey, minimum_bid: u64) -> Self {
        ActivateBidArgs {
            validator,
            minimum_bid,
        }
    }
}

pub fn activate_bid<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: ActivateBidArgs,
) -> Result<(), DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| runtime.activate_bid(args.validator.clone(), args.minimum_bid),
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "activate bid failed on dispatch");
            return Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_ACTIVATE_BID);

    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            error!(%error, ?args, "activate bid failed with error");
            Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ))
        }
    }
}
