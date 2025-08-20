//! Add Bid.

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::{
        auction,
        auction::{DelegationRate, METHOD_ADD_BID},
    },
    ApiError, PublicKey, TransactionHash, U512,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct AddBidArgs {
    public_key: PublicKey,
    delegation_rate: DelegationRate,
    amount: U512,
    minimum_delegation_amount: u64,
    maximum_delegation_amount: u64,
    minimum_bid_amount: u64,
    max_delegators_per_validator: u32,
    reserved_slots: u32,
}

impl AddBidArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
        minimum_bid_amount: u64,
        max_delegators_per_validator: u32,
        reserved_slots: u32,
    ) -> Self {
        AddBidArgs {
            public_key,
            delegation_rate,
            amount,
            minimum_delegation_amount,
            maximum_delegation_amount,
            minimum_bid_amount,
            max_delegators_per_validator,
            reserved_slots,
        }
    }
}

pub fn add_bid<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: AddBidArgs,
) -> Result<U512, DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.add_bid(
                args.public_key.clone(),
                args.delegation_rate,
                args.amount,
                args.minimum_delegation_amount,
                args.maximum_delegation_amount,
                args.minimum_bid_amount,
                args.max_delegators_per_validator,
                args.reserved_slots,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "add bid failed on dispatch");
            return Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_ADD_BID);
    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(error) => {
            if let ApiError::AuctionError(code) = error {
                if code == auction::Error::GasLimit as u8 {
                    return Err(DispatchError::Call(CallError::CalleeGasDepleted));
                }
            }
            error!(%error, ?args, "add bid failed with error");
            Err(DispatchError::Api(error))
        }
    }
}
