//! Withdraw.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};

use casper_types::{
    account::AccountHash, system::auction::METHOD_WITHDRAW_BID, ApiError, Key, PublicKey,
    RuntimeFootprint, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct WithdrawBidArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    public_key: PublicKey,
    amount: U512,
}

impl WithdrawBidArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_native_config: RuntimeNativeConfig,
        id: TransactionHash,
        address_generator: Arc<RwLock<AddressGenerator>>,
        initiator: AccountHash,
        context_key: Key,
        remaining_spending_limit: U512,
        public_key: PublicKey,
        amount: U512,
    ) -> Self {
        WithdrawBidArgs {
            address_generator,
            runtime_native_config,
            id,
            initiator,
            context_key,
            remaining_spending_limit,
            public_key,
            amount,
        }
    }
}

pub fn withdraw_bid<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: WithdrawBidArgs,
) -> Result<U512, DispatchError> {
    debug!(?args, METHOD_WITHDRAW_BID);

    let minimum_bid_amount = args.runtime_native_config.minimum_bid_amount();
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
            runtime.withdraw_bid(args.public_key.clone(), args.amount, minimum_bid_amount)
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "withdraw bid failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_WITHDRAW_BID);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, "withdraw bid failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
