//! Withdraw.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::METHOD_WITHDRAW_BID, ApiError, PublicKey, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct WithdrawBidArgs {
    public_key: PublicKey,
    amount: U512,
    minimum_bid_amount: u64,
}

impl WithdrawBidArgs {
    pub fn new(public_key: PublicKey, amount: U512, minimum_bid_amount: u64) -> Self {
        WithdrawBidArgs {
            public_key,
            amount,
            minimum_bid_amount,
        }
    }
}

pub fn withdraw_bid<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: WithdrawBidArgs,
) -> Result<U512, DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.withdraw_bid(
                args.public_key.clone(),
                args.amount,
                args.minimum_bid_amount,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "withdraw bid failed on dispatch");
            return Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_WITHDRAW_BID);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, ?args, "withdraw bid failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
