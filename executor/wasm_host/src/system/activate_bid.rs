//! Activate Bid.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash, system::auction::METHOD_ACTIVATE_BID, ApiError, Key, PublicKey,
    RuntimeFootprint, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct ActivateBidArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    validator: PublicKey,
}

impl ActivateBidArgs {
    pub fn new(
        runtime_native_config: RuntimeNativeConfig,
        id: TransactionHash,
        address_generator: Arc<RwLock<AddressGenerator>>,
        initiator: AccountHash,
        context_key: Key,
        remaining_spending_limit: U512,
        validator: PublicKey,
    ) -> Self {
        ActivateBidArgs {
            runtime_native_config,
            id,
            address_generator,
            initiator,
            context_key,
            remaining_spending_limit,
            validator,
        }
    }
}

pub fn activate_bid<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: ActivateBidArgs,
) -> Result<(), DispatchError> {
    debug!(?args, METHOD_ACTIVATE_BID);
    let minimum_bid = args.runtime_native_config.minimum_bid_amount();
    let result = match super::dispatch_userland_to_system_contract(
        tracking_copy,
        runtime_footprint,
        args.runtime_native_config,
        args.id,
        args.address_generator,
        args.initiator,
        args.context_key,
        args.remaining_spending_limit,
        |mut runtime| runtime.activate_bid(args.validator.clone(), minimum_bid),
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "activate bid failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_ACTIVATE_BID);
    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, "activate bid failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
