//! Change Bid Public Key.

use std::sync::Arc;

use crate::system::DispatchError;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::FatalHostError;
use casper_storage::{
    global_state::GlobalStateReader, system::auction::Auction, AddressGenerator,
    RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    system::auction::METHOD_CHANGE_BID_PUBLIC_KEY, ApiError, PublicKey, TransactionHash,
};
use parking_lot::RwLock;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct ChangeBidPublicKeyArgs {
    public_key: PublicKey,
    new_public_key: PublicKey,
}

impl ChangeBidPublicKeyArgs {
    pub fn new(public_key: PublicKey, new_public_key: PublicKey) -> Self {
        ChangeBidPublicKeyArgs {
            public_key,
            new_public_key,
        }
    }
}

pub fn change_bid_public_key<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: ChangeBidPublicKeyArgs,
) -> Result<(), DispatchError> {
    let result = match super::dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        id,
        address_generator,
        |mut runtime| {
            runtime.change_bid_public_key(args.public_key.clone(), args.new_public_key.clone())
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            error!(%error, "change bid public key failed on dispatch");
            return Err(DispatchError::Internal(
                FatalHostError::DispatchSystemContract,
            ));
        }
    };

    debug!(?args, ?result, METHOD_CHANGE_BID_PUBLIC_KEY);

    match result {
        Ok(_) => Ok(()),
        Err(casper_types::system::auction::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(error) => {
            let api_error: ApiError = error.into();
            error!(%api_error, ?args, "change bid public key failed with error");
            Err(DispatchError::Api(api_error))
        }
    }
}
