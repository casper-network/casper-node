//! Redelegate.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::{DelegatorKind, METHOD_REDELEGATE},
    PublicKey, TransactionHash, U512,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct RedelegateArgs {
    delegator_kind: DelegatorKind,
    validator_public_key: PublicKey,
    amount: U512,
    new_validator: PublicKey,
}

impl RedelegateArgs {
    pub fn new(
        delegator_kind: DelegatorKind,
        validator_public_key: PublicKey,
        amount: U512,
        new_validator: PublicKey,
    ) -> Self {
        RedelegateArgs {
            delegator_kind,
            validator_public_key,
            amount,
            new_validator,
        }
    }
}

pub fn redelegate<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: RedelegateArgs,
) -> Result<U512, DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.redelegate(
                args.delegator_kind.clone(),
                args.validator_public_key.clone(),
                args.amount,
                args.new_validator.clone(),
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

    debug!(?args, ?result, METHOD_REDELEGATE);

    match result {
        Ok(updated_amount) => Ok(updated_amount),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            error!(%error, ?args, "withdraw bid failed with error");
            Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ))
        }
    }
}
