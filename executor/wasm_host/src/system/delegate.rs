//! Delegate.

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
    system::{
        auction,
        auction::{DelegatorKind, METHOD_DELEGATE},
    },
    ApiError, Key, PublicKey, RuntimeFootprint, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct DelegateArgs {
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

impl DelegateArgs {
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
        DelegateArgs {
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

pub fn delegate<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_footprint: RuntimeFootprint,
    args: DelegateArgs,
) -> Result<U512, DispatchError> {
    debug!(?args, METHOD_DELEGATE);

    let max_delegators_per_validator = args.runtime_native_config.max_delegators_per_validator();
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
            runtime.delegate(
                args.delegator_kind.clone(),
                args.validator_public_key.clone(),
                args.amount,
                max_delegators_per_validator,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "delegate failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?result, METHOD_DELEGATE);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(error) => {
            if let ApiError::AuctionError(code) = error {
                if code == auction::Error::GasLimit as u8 {
                    return Err(DispatchError::Call(CallError::CalleeGasDepleted));
                }
            }
            error!(%error, "delegate failed with error");
            Err(DispatchError::Api(error))
        }
    }
}
