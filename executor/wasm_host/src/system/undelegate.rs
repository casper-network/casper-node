//! Undelegate.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::{DelegatorKind, METHOD_UNDELEGATE},
    ApiError, PublicKey, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct UndelegateArgs {
    delegator_kind: DelegatorKind,
    validator_public_key: PublicKey,
    amount: U512,
}

impl UndelegateArgs {
    pub fn new(
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Self {
        UndelegateArgs {
            delegator_kind,
            validator_public_key,
            amount,
        }
    }
}

pub fn undelegate<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: UndelegateArgs,
) -> Result<U512, DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.undelegate(
                args.delegator_kind.clone(),
                args.validator_public_key.clone(),
                args.amount,
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

    debug!(?args, ?result, METHOD_UNDELEGATE);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, ?args, "undelegate failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
