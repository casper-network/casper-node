//! Delegate.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::{DelegatorKind, METHOD_DELEGATE},
    ApiError, PublicKey, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct DelegateArgs {
    delegator_kind: DelegatorKind,
    validator_public_key: PublicKey,
    amount: U512,
    max_delegators_per_validator: u32,
}

impl DelegateArgs {
    pub fn new(
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
        max_delegators_per_validator: u32,
    ) -> Self {
        DelegateArgs {
            delegator_kind,
            validator_public_key,
            amount,
            max_delegators_per_validator,
        }
    }
}

pub fn delegate<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: DelegateArgs,
) -> Result<U512, DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.delegate(
                args.delegator_kind.clone(),
                args.validator_public_key.clone(),
                args.amount,
                args.max_delegators_per_validator,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "undelegate failed on dispatch");
            return Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_DELEGATE);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(ApiError::AuctionError(code)) => {
            error!(%code, ?args, "delegate failed with error code");

            if code == 40 {
                return Err(DispatchError::Call(CallError::CalleeGasDepleted));
            }

            Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ))
        }
        Err(error) => {
            error!(%error, ?args, "delegate failed with error");

            Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ))
        }
    }
}
