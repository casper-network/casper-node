//! Undelegate.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    system::auction::{DelegatorKind, METHOD_UNDELEGATE},
    ApiError, Key, PublicKey, RuntimeFootprint, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct UndelegateArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    delegator_kind: DelegatorKind,
    validator_public_key: PublicKey,
    amount: U512,
}

impl UndelegateArgs {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_native_config: RuntimeNativeConfig,
        id: TransactionHash,
        address_generator: Arc<RwLock<AddressGenerator>>,
        initiator: AccountHash,
        context_key: Key,
        remaining_spending_limit: U512,
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Self {
        UndelegateArgs {
            address_generator,
            runtime_native_config,
            id,
            initiator,
            context_key,
            remaining_spending_limit,
            delegator_kind,
            validator_public_key,
            amount,
        }
    }
}

pub fn undelegate<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: UndelegateArgs,
) -> Result<U512, DispatchError> {
    debug!(?args, METHOD_UNDELEGATE);

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
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_UNDELEGATE);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, "undelegate failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
