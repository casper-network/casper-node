//! Create Purse.

use std::sync::Arc;

use casper_executor_wasm_interface::{FatalHostError, VMError, VMResult};
use casper_storage::{
    global_state::GlobalStateReader, system::mint::Mint, AddressGenerator, RuntimeNativeConfig,
    TrackingCopy,
};
use casper_types::{TransactionHash, URef, U512};
use parking_lot::RwLock;
use tracing::error;

use crate::system::dispatch_system_contract;

pub fn create_purse<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
) -> VMResult<URef> {
    let mint_result = match dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        transaction_hash,
        address_generator,
        |mut runtime| runtime.mint(U512::zero()),
    ) {
        Ok(mint_result) => mint_result,
        Err(error) => {
            error!(%error, "create purse failed on dispatch");
            return Err(VMError::Fatal(FatalHostError::DispatchSystemContract));
        }
    };

    match mint_result {
        Ok(uref) => Ok(uref),
        Err(casper_types::system::mint::Error::GasLimit) => Err(VMError::OutOfGas),
        Err(mint_error) => {
            error!(%mint_error, "create purse failed with error");
            Err(VMError::Fatal(FatalHostError::DispatchSystemContract))
        }
    }
}
