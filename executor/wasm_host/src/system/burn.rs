//! Burn.

use std::sync::Arc;

use crate::system::{dispatch_system_contract, DispatchError};
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::mint::Mint, AddressGenerator, RuntimeNativeConfig,
    TrackingCopy,
};
use casper_types::{system::mint::METHOD_BURN, TransactionHash, URef, U512};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Copy, Clone)]
pub struct BurnArgs {
    purse: URef,
    amount: U512,
}

impl BurnArgs {
    pub fn new(purse: URef, amount: U512) -> Self {
        BurnArgs { purse, amount }
    }
}

pub fn burn<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: BurnArgs,
) -> Result<(), DispatchError> {
    let result = match dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        transaction_hash,
        address_generator,
        |mut runtime| runtime.burn(args.purse, args.amount),
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "burn failed on dispatch");
            return Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_BURN);

    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::mint::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            error!(%error, ?args, "burn failed with error");
            Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ))
        }
    }
}
