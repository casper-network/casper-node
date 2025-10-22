//! Redelegate.

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
    system::auction::{DelegatorKind, METHOD_REDELEGATE},
    ApiError, Key, PublicKey, RuntimeFootprint, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct RedelegateArgs {
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    delegator_kind: DelegatorKind,
    validator_public_key: PublicKey,
    amount: U512,
    new_validator_public_key: PublicKey,
}

impl RedelegateArgs {
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
        new_validator_public_key: PublicKey,
    ) -> Self {
        RedelegateArgs {
            address_generator,
            runtime_native_config,
            id,
            initiator,
            context_key,
            remaining_spending_limit,
            delegator_kind,
            validator_public_key,
            amount,
            new_validator_public_key,
        }
    }
}

pub fn redelegate<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: RedelegateArgs,
) -> Result<U512, DispatchError> {
    debug!(?args, METHOD_REDELEGATE);

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
            runtime.redelegate(
                args.delegator_kind.clone(),
                args.validator_public_key.clone(),
                args.amount,
                args.new_validator_public_key.clone(),
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "redelegate failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_REDELEGATE);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, "redelegate failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
