//! System contract dispatch.
//!
//! System contracts are special contracts that are always available to the system.
//! They are used to implement core system functionality, such as minting and transferring tokens.
//! This module provides a way to dispatch calls to system contracts that are implemented under
//! storage crate.
//!
//! The dispatcher provides the necessary information to properly execute system contract's code
//! within the context of the current execution of the new Wasm host logic.
use std::{cell::RefCell, rc::Rc, sync::Arc};

use casper_executor_wasm_common::error::{CallError, TrapCode};
use casper_executor_wasm_interface::HostResult;
use casper_storage::{
    global_state::GlobalStateReader,
    system::{
        mint::Mint,
        runtime_native::{Id, RuntimeNative},
    },
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    account::AccountHash, system::MINT, CLValueError, ContextAccessRights, EntityAddr, Key, Phase,
    PublicKey, SystemHashRegistry, TransactionHash, URef, METHOD_TRANSFER, U512,
};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, error};

#[derive(Debug, Error)]
enum DispatchError {
    #[error("Tracking copy error: {0}")]
    Storage(#[from] TrackingCopyError),
    #[error("CLValue error: {0}")]
    CLValue(CLValueError),
    #[error("Registry not found")]
    RegistryNotFound,
    #[error("Missing addressable entity")]
    MissingRuntimeFootprint(TrackingCopyError),
    #[error("Missing system contract: {0}")]
    MissingSystemContract(&'static str),
}

fn dispatch_system_contract<R: GlobalStateReader, Ret>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    system_contract: &'static str,
    func: impl FnOnce(RuntimeNative<R>) -> Ret,
) -> Result<Ret, DispatchError> {
    let system_entity_registry = {
        let stored_value = tracking_copy
            .read(&Key::SystemEntityRegistry)?
            .ok_or(DispatchError::RegistryNotFound)?;
        stored_value
            .into_cl_value()
            .expect("should convert stored value into CLValue")
            .into_t::<SystemHashRegistry>()
            .map_err(DispatchError::CLValue)?
    };
    let system_entity_addr = system_entity_registry
        .get(system_contract)
        .ok_or(DispatchError::MissingSystemContract(system_contract))?;
    let entity_addr = EntityAddr::new_system(*system_entity_addr);

    let runtime_footprint = tracking_copy
        .runtime_footprint_by_entity_addr(entity_addr)
        .map_err(DispatchError::MissingRuntimeFootprint)?;

    let access_rights = ContextAccessRights::new(*system_entity_addr, []);
    let address = PublicKey::System.to_account_hash();

    let forked_tracking_copy = Rc::new(RefCell::new(tracking_copy.fork2()));

    let remaining_spending_limit = U512::MAX; // NOTE: Since there's no custom payment, there's no need to track the remaining spending limit.
    let phase = Phase::System; // NOTE: Since this is a system contract, the phase is always `System`.

    let ret = {
        let runtime = RuntimeNative::new(
            runtime_native_config,
            Id::Transaction(transaction_hash),
            address_generator,
            Rc::clone(&forked_tracking_copy),
            address,
            Key::AddressableEntity(entity_addr),
            runtime_footprint,
            access_rights,
            remaining_spending_limit,
            phase,
        );

        func(runtime)
    };

    // SAFETY: `RuntimeNative` is dropped in the block above, we can extract the tracking copy and
    // the effects.
    let modified_tracking_copy = Rc::try_unwrap(forked_tracking_copy)
        .ok()
        .expect("No other references");

    let modified_tracking_copy = modified_tracking_copy.into_inner();

    tracking_copy.apply_changes(
        modified_tracking_copy.effects(),
        modified_tracking_copy.cache(),
        modified_tracking_copy.messages(),
    );

    Ok(ret)
}

pub(crate) fn create_purse<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
) -> Result<URef, CallError> {
    let mint_result = match dispatch_system_contract(
        tracking_copy,
        runtime_native_config,
        transaction_hash,
        address_generator,
        MINT,
        |mut runtime| runtime.mint(U512::zero()),
    ) {
        Ok(mint_result) => mint_result,
        Err(error) => {
            error!(%error, "create purse failed on dispatch");
            return Err(CallError::CalleeTrapped(TrapCode::NativeDispatchFailure));
        }
    };

    match mint_result {
        Ok(uref) => Ok(uref),
        Err(casper_types::system::mint::Error::InsufficientFunds) => Err(CallError::CalleeReverted),
        Err(casper_types::system::mint::Error::GasLimit) => Err(CallError::CalleeGasDepleted),
        Err(mint_error) => {
            error!(%mint_error, "create purse failed with error");
            Err(CallError::CalleeTrapped(TrapCode::NativeError))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct MintTransferArgs {
    maybe_to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
}

impl MintTransferArgs {
    pub(crate) fn new_simple(source: URef, target: URef, amount: U512) -> Self {
        MintTransferArgs {
            source,
            target,
            amount,
            maybe_to: None,
            id: None,
        }
    }
}

pub(crate) fn transfer<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: MintTransferArgs,
) -> HostResult {
    let transfer_result: Result<(), casper_types::system::mint::Error> =
        match dispatch_system_contract(
            tracking_copy,
            runtime_native_config,
            id,
            address_generator,
            MINT,
            |mut runtime| {
                runtime.transfer(
                    args.maybe_to,
                    args.source,
                    args.target,
                    args.amount,
                    args.id,
                )
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                error!(%error, "transfer failed on dispatch");
                return Err(CallError::CalleeTrapped(TrapCode::NativeDispatchFailure));
            }
        };

    debug!(?args, ?transfer_result, METHOD_TRANSFER);

    match transfer_result {
        Ok(()) => Ok(()),
        Err(casper_types::system::mint::Error::InsufficientFunds) => Err(CallError::CalleeReverted),
        Err(casper_types::system::mint::Error::GasLimit) => Err(CallError::CalleeGasDepleted),
        Err(mint_error) => {
            error!(%mint_error, ?args, "transfer failed with error");
            Err(CallError::CalleeTrapped(TrapCode::NativeError))
        }
    }
}
