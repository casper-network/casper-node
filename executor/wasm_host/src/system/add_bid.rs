//! Add Bid.

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    system::{
        auction,
        auction::{DelegationRate, METHOD_ADD_BID},
    },
    ApiError, Key, PublicKey, RuntimeFootprint, TransactionHash, U512,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct AddBidArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    public_key: PublicKey,
    delegation_rate: DelegationRate,
    amount: U512,
    minimum_delegation_amount: u64,
    maximum_delegation_amount: u64,
    reserved_slots: u32,
}

impl AddBidArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_native_config: RuntimeNativeConfig,
        id: TransactionHash,
        address_generator: Arc<RwLock<AddressGenerator>>,
        initiator: AccountHash,
        context_key: Key,
        remaining_spending_limit: U512,

        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
        reserved_slots: u32,
    ) -> Self {
        AddBidArgs {
            runtime_native_config,
            id,
            address_generator,
            initiator,
            context_key,
            remaining_spending_limit,
            public_key,
            delegation_rate,
            amount,
            minimum_delegation_amount,
            maximum_delegation_amount,
            reserved_slots,
        }
    }
}

pub fn add_bid<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: AddBidArgs,
) -> Result<U512, DispatchError> {
    debug!(?args, METHOD_ADD_BID);
    let vesting = args.runtime_native_config.vesting_schedule_period_millis();
    let max_delegators_per_validator = args.runtime_native_config.max_delegators_per_validator();
    let min_bid_amount = args.runtime_native_config.minimum_bid_amount();
    let result = match super::dispatch_userland_to_system_contract(
        tracking_copy,
        runtime_footprint,
        args.runtime_native_config,
        args.id,
        args.address_generator,
        args.initiator,
        args.context_key,
        args.remaining_spending_limit,
        |mut runtime| {
            runtime.add_bid(
                args.public_key.clone(),
                args.delegation_rate,
                args.amount,
                vesting,
                args.minimum_delegation_amount,
                args.maximum_delegation_amount,
                min_bid_amount,
                max_delegators_per_validator,
                args.reserved_slots,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "add bid failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_ADD_BID);
    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(error) => {
            if let ApiError::AuctionError(code) = error {
                if code == auction::Error::GasLimit as u8 {
                    return Err(DispatchError::Call(CallError::CalleeGasDepleted));
                }
            }
            error!(%error, "add bid failed with error");
            Err(DispatchError::Api(error))
        }
    }
}
