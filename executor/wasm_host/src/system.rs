//! System contract wire up for the new engine.
//!
//! This module wraps system contract logic into a dispatcher that can be used by the new engine
//! hiding the complexity of the underlying implementation.

mod activate_bid;
mod add_bid;
mod add_reservations;
mod burn;
mod cancel_reservations;
mod change_bid_public_key;
mod create_purse;
mod delegate;
mod redelegate;
mod transfer;
mod undelegate;
mod withdraw_bid;

use bytes::Bytes;
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::{executor::SystemContractMenu, FatalHostError, GasUsage};
use casper_storage::{
    global_state::GlobalStateReader,
    system::runtime_native::{Id, RuntimeNative},
    tracking_copy::TrackingCopyError,
    AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    bytesrepr, AccessRights, ApiError, CLValueError, EntityAddr, Key, Phase, PublicKey,
    RuntimeFootprint, TransactionHash, URef, URefAddr, U512,
};
use parking_lot::RwLock;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use thiserror::Error;
use tracing::{debug, error};

use casper_executor_wasm_interface::executor::{
    AuctionMethods, ExecuteError, ExecuteResult, MintMethods,
};
use casper_types::bytesrepr::ToBytes;

use crate::system;
use casper_types::system::auction::{
    DelegatorKind, Reservation, DELEGATION_RATE_DENOMINATOR, ERA_END_TIMESTAMP_MILLIS_KEY,
    ERA_ID_KEY,
};

pub use activate_bid::{activate_bid, ActivateBidArgs};
pub use add_bid::{add_bid, AddBidArgs};
pub use add_reservations::{add_reservations, AddReservationsArgs};
pub use burn::{burn, BurnArgs};
pub use cancel_reservations::{cancel_reservations, CancelReservationsArgs};
use casper_storage::tracking_copy::{TrackingCopyEntityExt, TrackingCopyExt};
use casper_types::{
    account::AccountHash,
    system::{mint::TOTAL_SUPPLY_KEY, AUCTION, MINT},
};
pub use change_bid_public_key::{change_bid_public_key, ChangeBidPublicKeyArgs};
pub use create_purse::create_purse;
pub use delegate::{delegate, DelegateArgs};
pub use redelegate::{redelegate, RedelegateArgs};
pub use transfer::{transfer, TransferArgs};
pub use undelegate::{undelegate, UndelegateArgs};
pub use withdraw_bid::{withdraw_bid, WithdrawBidArgs};

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("Tracking copy error: {0}")]
    Storage(TrackingCopyError),
    #[error("CLValue error: {0}")]
    CLValue(CLValueError),
    #[error("Registry not found")]
    RegistryNotFound,
    #[error("Missing system contract: {0}")]
    MissingSystemContract(String),
    #[error("Runtime footprint")]
    RuntimeFootprint(TrackingCopyError),
    // INTERNAL ERRORS ARE FATAL!
    #[error("Internal host error: {0}")]
    Internal(FatalHostError),
    #[error("Call error: {0}")]
    Call(CallError),
    #[error("Api error: {0}")]
    Api(ApiError),
}

#[allow(clippy::too_many_arguments)]
fn dispatch_userland_to_system_contract<R: GlobalStateReader, Ret: PartialEq>(
    tracking_copy: &mut TrackingCopy<R>,
    mut runtime_footprint: RuntimeFootprint,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    initiator: AccountHash,
    context_key: Key,
    remaining_spending_limit: U512,
    func: impl FnOnce(RuntimeNative<R>) -> Ret,
) -> Result<Ret, DispatchError> {
    let forked_tracking_copy = Rc::new(RefCell::new(tracking_copy.fork2()));

    let mut access_rights = runtime_footprint.extract_access_rights();
    match tracking_copy.system_contract_named_key(MINT, TOTAL_SUPPLY_KEY) {
        Ok(Some(k)) => {
            match k.as_uref() {
                Some(uref) => access_rights.extend(&[*uref]),
                None => {
                    return Err(DispatchError::Storage(
                        TrackingCopyError::UnexpectedKeyVariant(k),
                    ));
                }
            }
            runtime_footprint.insert_into_named_keys(TOTAL_SUPPLY_KEY.into(), k);
        }
        Ok(None) => {
            return Err(DispatchError::Storage(TrackingCopyError::NamedKeyNotFound(
                TOTAL_SUPPLY_KEY.into(),
            )));
        }
        Err(tce) => {
            return Err(DispatchError::Storage(tce));
        }
    };

    match tracking_copy.system_contract_named_key(AUCTION, ERA_END_TIMESTAMP_MILLIS_KEY) {
        Ok(Some(k)) => {
            match k.as_uref() {
                Some(uref) => access_rights.extend(&[*uref]),
                None => {
                    return Err(DispatchError::Storage(
                        TrackingCopyError::UnexpectedKeyVariant(k),
                    ));
                }
            }
            runtime_footprint.insert_into_named_keys(ERA_END_TIMESTAMP_MILLIS_KEY.into(), k);
        }
        Ok(None) => {
            return Err(DispatchError::Storage(TrackingCopyError::NamedKeyNotFound(
                ERA_END_TIMESTAMP_MILLIS_KEY.into(),
            )));
        }
        Err(tce) => {
            return Err(DispatchError::Storage(tce));
        }
    };
    match tracking_copy.system_contract_named_key(AUCTION, ERA_ID_KEY) {
        Ok(Some(k)) => {
            match k.as_uref() {
                Some(uref) => access_rights.extend(&[*uref]),
                None => {
                    return Err(DispatchError::Storage(
                        TrackingCopyError::UnexpectedKeyVariant(k),
                    ));
                }
            }
            runtime_footprint.insert_into_named_keys(ERA_ID_KEY.into(), k);
        }
        Ok(None) => {
            return Err(DispatchError::Storage(TrackingCopyError::NamedKeyNotFound(
                ERA_END_TIMESTAMP_MILLIS_KEY.into(),
            )));
        }
        Err(tce) => {
            return Err(DispatchError::Storage(tce));
        }
    };

    let ret = {
        let runtime = RuntimeNative::new(
            runtime_native_config,
            Id::Transaction(transaction_hash),
            address_generator,
            Rc::clone(&forked_tracking_copy),
            initiator,
            context_key,
            runtime_footprint,
            access_rights,
            remaining_spending_limit,
            Phase::Session,
        );

        func(runtime)
    };

    // SAFETY: `RuntimeNative` is dropped in the block above, we can extract the tracking copy's
    // effects.
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

fn dispatch_system_contract<R: GlobalStateReader, Ret: PartialEq>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    func: impl FnOnce(RuntimeNative<R>) -> Ret,
) -> Result<Ret, DispatchError> {
    let forked_tracking_copy = Rc::new(RefCell::new(tracking_copy.fork2()));
    let ret = {
        let runtime = RuntimeNative::new_system_runtime(
            runtime_native_config,
            Id::Transaction(transaction_hash),
            address_generator,
            Rc::clone(&forked_tracking_copy),
            Phase::System,
        )
        .map_err(|tracking_copy_error| {
            error!(%tracking_copy_error, "Failed to create system contract runtime");
            DispatchError::Internal(FatalHostError::DispatchSystemContract)
        })?;

        func(runtime)
    };

    let modified_tracking_copy = Rc::try_unwrap(forked_tracking_copy).map_err(|_| {
        // SAFETY: `RuntimeNative` is dropped in the block above, we can extract the tracking copy
        // the effects.
        error!("Expected the tracking copy to have no other references");
        DispatchError::Internal(FatalHostError::TypeConversion)
    })?;

    let modified_tracking_copy = modified_tracking_copy.into_inner();

    tracking_copy.apply_changes(
        modified_tracking_copy.effects(),
        modified_tracking_copy.cache(),
        modified_tracking_copy.messages(),
    );

    Ok(ret)
}

/// This function adapts inner system contract interactions into direct execution using
/// ExecuteRequest / ExecuteResult / ExecuteError semantics.
///
/// This is intended to interface with VM based calls.
/// Inner logic that needs to interact with system contract(s) should instead call the appropriate
/// system function(s) directly.
#[allow(clippy::too_many_arguments)]
pub fn native_exec<A, T: ToBytes, R: GlobalStateReader + 'static>(
    mut tracking_copy: TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    gas_usage: GasUsage,
    initiator: AccountHash,
    caller_key: Key,
    input: Bytes,
    system_menu_selection: SystemContractMenu,
) -> Result<ExecuteResult, ExecuteError> {
    let (caller_key, entity_addr) = if let Key::Account(account_hash) = caller_key {
        (caller_key, EntityAddr::Account(account_hash.value()))
    } else if let Key::Hash(contract_hash_addr) = caller_key {
        match tracking_copy.get_package(contract_hash_addr) {
            Ok(package) => match package.enabled_versions().latest() {
                Some(entity_addr) => (Key::Hash(entity_addr.value()), *entity_addr),
                None => {
                    return Ok(ExecuteResult {
                        host_error: Some(CallError::NoActiveContract),
                        output: None,
                        gas_usage,
                        effects: tracking_copy.effects(),
                        cache: tracking_copy.cache(),
                        messages: tracking_copy.messages(),
                    })
                }
            },
            Err(tce) => return Err(ExecuteError::Api(tce.to_string())),
        }
    } else if let Key::Package(package_addr) = caller_key {
        match tracking_copy.get_package(package_addr.value()) {
            Ok(package) => match package.enabled_versions().latest() {
                Some(entity_addr) => (Key::Hash(entity_addr.value()), *entity_addr),
                None => {
                    return Ok(ExecuteResult {
                        host_error: Some(CallError::NoActiveContract),
                        output: None,
                        gas_usage,
                        effects: tracking_copy.effects(),
                        cache: tracking_copy.cache(),
                        messages: tracking_copy.messages(),
                    })
                }
            },
            Err(tce) => return Err(ExecuteError::Api(tce.to_string())),
        }
    } else if let Key::AddressableEntity(entity_addr) = caller_key {
        (caller_key, entity_addr)
    } else {
        return Ok(ExecuteResult {
            host_error: Some(CallError::EntityNotFound),
            output: None,
            gas_usage,
            effects: tracking_copy.effects(),
            cache: tracking_copy.cache(),
            messages: tracking_copy.messages(),
        });
    };

    let runtime_footprint = match tracking_copy.runtime_footprint_by_entity_addr(entity_addr) {
        Ok(footprint) => footprint,
        Err(err) => {
            debug!(
                ?err,
                ?entity_addr,
                "native_exec failed attempt to runtime_footprint_by_entity_addr"
            );
            return Ok(ExecuteResult {
                host_error: Some(CallError::EntityNotFound),
                output: None,
                gas_usage,
                effects: tracking_copy.effects(),
                cache: tracking_copy.cache(),
                messages: tracking_copy.messages(),
            });
        }
    };

    let ret: Result<Option<Bytes>, DispatchError> = match system_menu_selection {
        SystemContractMenu::Auction(method) => match method {
            AuctionMethods::Activate => {
                let ret = bytesrepr::deserialize_from_slice::<&Bytes, (PublicKey,)>(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Activate");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                if unpacked.0.is_system() {
                    debug!(
                        ?method,
                        "attempt to pass system public key from userland Activate"
                    );
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                let args = ActivateBidArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    unpacked.0,
                );
                match system::activate_bid(&mut tracking_copy, runtime_footprint, args) {
                    Ok(_) => Ok(None),
                    Err(de) => Err(de),
                }
            }
            AuctionMethods::Bid => {
                let ret = bytesrepr::deserialize_from_slice::<
                    &Bytes,
                    (PublicKey, u8, u64, u64, u64, u32),
                >(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Bid");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                if unpacked.0.is_system() {
                    debug!(
                        ?method,
                        "attempt to pass system public key from userland Bid"
                    );
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                let delegation_rate = {
                    if unpacked.1 > DELEGATION_RATE_DENOMINATOR {
                        DELEGATION_RATE_DENOMINATOR
                    } else {
                        unpacked.1
                    }
                };
                let min_del_amount = {
                    if unpacked.3 < runtime_native_config.minimum_delegation_amount() {
                        runtime_native_config.minimum_delegation_amount()
                    } else {
                        unpacked.3
                    }
                };
                let max_del_amount = {
                    if unpacked.4 > runtime_native_config.maximum_delegation_amount() {
                        runtime_native_config.maximum_delegation_amount()
                    } else {
                        unpacked.4
                    }
                };
                let args = AddBidArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    // public_key
                    unpacked.0,
                    delegation_rate,
                    // amount
                    unpacked.2.into(),
                    // minimum_delegation_amount
                    min_del_amount,
                    max_del_amount,
                    // reserved_slots
                    unpacked.5,
                );
                match system::add_bid(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Withdraw => {
                let unpacked: (PublicKey, u64) = bytesrepr::deserialize_from_slice(&input)
                    .map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                if unpacked.0.is_system() {
                    debug!(
                        ?method,
                        "attempt to pass system public key from userland Withdraw"
                    );
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                let args = WithdrawBidArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    unpacked.0,
                    unpacked.1.into(),
                );
                match system::withdraw_bid(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Delegate => {
                let ret = bytesrepr::deserialize_from_slice::<
                    &Bytes,
                    (DelegatorKind, PublicKey, u64),
                >(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Delegate");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                if let DelegatorKind::PublicKey(del_pub_key) = &unpacked.0 {
                    if del_pub_key.is_system() {
                        debug!(
                            ?method,
                            "attempt to pass system public key from userland Delegate source"
                        );
                        return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                    }
                }
                if unpacked.1.is_system() {
                    debug!(
                        ?method,
                        "attempt to pass system public key from userland Delegate target"
                    );
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                let args = DelegateArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    // delegator_kind
                    unpacked.0,
                    // validator_public_key
                    unpacked.1,
                    // amount
                    unpacked.2.into(),
                );
                match system::delegate(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Undelegate => {
                let ret = bytesrepr::deserialize_from_slice::<
                    &Bytes,
                    (DelegatorKind, PublicKey, u64),
                >(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Undelegate");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let args = UndelegateArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    // delegator_kind
                    unpacked.0,
                    // validator_public_key
                    unpacked.1,
                    // amount
                    unpacked.2.into(),
                );
                match system::undelegate(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Redelegate => {
                let ret = bytesrepr::deserialize_from_slice::<
                    &Bytes,
                    (DelegatorKind, PublicKey, u64, PublicKey),
                >(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Redelegate");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let args = RedelegateArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    // delegator_kind
                    unpacked.0,
                    // validator_public_key
                    unpacked.1,
                    // amount
                    unpacked.2.into(),
                    // new_validator_public_key
                    unpacked.3,
                );
                match system::redelegate(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::AddReservation => {
                let ret = bytesrepr::deserialize_from_slice::<&Bytes, (Reservation,)>(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec AddReservation");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let reservation = unpacked.0;
                if reservation.validator_public_key().is_system() {
                    debug!(
                        ?method,
                        "attempt to pass system public key from userland AddReservation validator"
                    );
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                if let DelegatorKind::PublicKey(delegator_public_key) = reservation.delegator_kind()
                {
                    if delegator_public_key.is_system() {
                        debug!(
                            ?method,
                            "attempt to pass system public key from userland AddReservation delegator"
                        );
                        return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                    }
                }
                let add_reservations_args = AddReservationsArgs::new(vec![reservation]);
                system::add_reservations(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    add_reservations_args,
                )
                .map(|_| None)
            }
            AuctionMethods::CancelReservation => {
                let unpacked: (PublicKey, DelegatorKind) =
                    bytesrepr::deserialize_from_slice(&input)
                        .map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                if unpacked.0.is_system() {
                    debug!(
                        ?method,
                        "attempt to pass system public key from userland CancelReservation"
                    );
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                let reservations = vec![unpacked.1];
                let cancel_reservations_args = CancelReservationsArgs::new(
                    // validator
                    unpacked.0,
                    // delegator
                    reservations,
                    runtime_native_config.max_delegators_per_validator(),
                );
                system::cancel_reservations(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    cancel_reservations_args,
                )
                .map(|_| None)
            }
            AuctionMethods::ChangePublicKey => {
                let ret =
                    bytesrepr::deserialize_from_slice::<&Bytes, (PublicKey, PublicKey)>(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec ChangePublicKey");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let pk_curr = unpacked.0;
                let pk_new = unpacked.1;
                if pk_curr.is_system() || pk_new.is_system() {
                    debug!(?method, "attempt to pass system public key from userland");
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidPublicKey));
                }
                let args = ChangeBidPublicKeyArgs::new(pk_curr, pk_new);
                system::change_bid_public_key(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                )
                .map(|_| None)
            }
        },
        SystemContractMenu::Mint(method) => match method {
            MintMethods::Burn => {
                // VM2 only allows userland burning from caller's main purse
                let ret = bytesrepr::deserialize_from_slice::<&Bytes, (u64,)>(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Burn");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let source = match tracking_copy.main_purse_by_key(&caller_key) {
                    Ok(uref) => uref,
                    Err(err) => return Err(ExecuteError::Api(err.to_string())),
                };
                let burn_amount = unpacked.0.into();
                let args = BurnArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    source,
                    burn_amount,
                );
                match system::burn(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            MintMethods::Transfer => {
                let ret = bytesrepr::deserialize_from_slice::<&Bytes, (EntityAddr, u64)>(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Transfer");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let target_entity = unpacked.0;
                if target_entity.is_system() {
                    debug!("attempt to pass system address from userland");
                    return Err(ExecuteError::Fatal(FatalHostError::InvalidEntityAddr));
                }
                let target = match tracking_copy.runtime_footprint_by_entity_addr(target_entity) {
                    Ok(target_runtime_footprint) => match target_runtime_footprint.main_purse() {
                        Some(target_purse) => URef::new(target_purse.addr(), AccessRights::ADD),
                        None => {
                            return Err(ExecuteError::Fatal(FatalHostError::UnexpectedEntityKind))
                        }
                    },
                    Err(err) => {
                        debug!(
                            ?err,
                            ?target_entity,
                            "runtime_footprint_by_entity_addr failed"
                        );
                        return Err(ExecuteError::Fatal(FatalHostError::TrackingCopy));
                    }
                };
                let source = match tracking_copy.main_purse_by_key(&caller_key) {
                    Ok(uref) => uref,
                    Err(err) => return Err(ExecuteError::Api(err.to_string())),
                };
                let args = TransferArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    source,
                    target,
                    // amount
                    unpacked.1.into(),
                );
                match system::transfer(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            MintMethods::TransferPurse => {
                let ret = bytesrepr::deserialize_from_slice::<&Bytes, (URefAddr, u64)>(&input);
                if let Err(err) = &ret {
                    debug!(?err, "bytesrepr error in native_exec Transfer");
                }
                let unpacked =
                    ret.map_err(|_err| ExecuteError::Fatal(FatalHostError::TypeConversion))?;
                let source = match tracking_copy.main_purse_by_key(&caller_key) {
                    Ok(uref) => uref,
                    Err(err) => return Err(ExecuteError::Api(err.to_string())),
                };
                let target = URef::new(unpacked.0, AccessRights::ADD);
                let args = TransferArgs::new(
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    initiator,
                    caller_key,
                    gas_usage.remaining_points().into(),
                    source,
                    target,
                    // amount
                    unpacked.1.into(),
                );
                match system::transfer(&mut tracking_copy, runtime_footprint, args) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
        },
    };

    let (output, host_error, execute_error) = match ret {
        Ok(maybe_bytes) => (maybe_bytes, None, None),
        Err(der) => match der {
            DispatchError::Api(apr) => {
                debug!(?transaction_hash, %apr, "api error");
                (None, Some(CallError::Api(apr.to_string())), None)
            }
            DispatchError::Call(cer) => {
                debug!(?transaction_hash, %cer, "call error");
                (None, Some(cer), None)
            }
            DispatchError::CLValue(cve) => {
                debug!(?transaction_hash, %cve, "cl value error");
                (None, Some(CallError::Api(cve.to_string())), None)
            }
            // the below are all node killers
            DispatchError::RegistryNotFound => {
                error!(?transaction_hash, "system contract registry not found");
                (
                    None,
                    None,
                    Some(ExecuteError::Fatal(FatalHostError::DispatchSystemContract)),
                )
            }
            DispatchError::MissingSystemContract(name) => {
                error!(?transaction_hash, ?name, "system contract not found");
                (
                    None,
                    None,
                    Some(ExecuteError::Fatal(FatalHostError::DispatchSystemContract)),
                )
            }
            DispatchError::Internal(ihe) => {
                error!(?transaction_hash, %ihe, "internal host error");
                (None, None, Some(ExecuteError::Fatal(ihe)))
            }
            DispatchError::Storage(tce) | DispatchError::RuntimeFootprint(tce) => {
                error!(?transaction_hash, %tce, "tracking copy error");
                (
                    None,
                    None,
                    Some(ExecuteError::Fatal(FatalHostError::TrackingCopy)),
                )
            }
        },
    };

    match execute_error {
        None => Ok(ExecuteResult {
            host_error,
            output,
            gas_usage,
            effects: tracking_copy.effects(),
            cache: tracking_copy.cache(),
            messages: tracking_copy.messages(),
        }),
        Some(exr) => Err(exr),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use casper_storage::{
        data_access_layer::{GenesisRequest, GenesisResult},
        global_state::{
            self,
            state::{CommitProvider, StateProvider},
        },
        system::{
            mint::{storage_provider::StorageProvider, Mint},
            runtime_native::Id,
        },
        AddressGenerator, RuntimeNativeConfig,
    };
    use casper_types::{
        ChainspecRegistry, Digest, GenesisConfig, Phase, ProtocolVersion, StorageCosts,
        SystemConfig, Timestamp, TransactionHash, TransactionV1Hash, WasmConfig, U512,
    };
    use parking_lot::RwLock;

    use crate::system::dispatch_system_contract;

    #[test]
    fn test_system_dispatcher() {
        let (global_state, initial_root_hash, _tempdir) =
            global_state::state::lmdb::make_temporary_global_state([]);

        let genesis_config = GenesisConfig::new(
            vec![],
            WasmConfig::default(),
            SystemConfig::default(),
            10,
            10,
            0,
            Default::default(),
            14,
            Timestamp::now().millis(),
            casper_types::HoldBalanceHandling::Accrued,
            0,
            false,
            StorageCosts::default(),
        );

        let genesis_request: GenesisRequest = GenesisRequest::new(
            Digest::hash("foo"),
            ProtocolVersion::V2_0_0,
            genesis_config,
            ChainspecRegistry::new_with_genesis(b"", b""),
        );

        let root_hash = match global_state.genesis(genesis_request) {
            GenesisResult::Failure(failure) => panic!("Failed to run genesis: {:?}", failure),
            GenesisResult::Fatal(fatal) => panic!("Fatal error while running genesis: {}", fatal),
            GenesisResult::Success {
                post_state_hash,
                effects: _,
            } => post_state_hash,
        };

        assert_ne!(
            root_hash, initial_root_hash,
            "Genesis should change the root hash"
        );

        let mut tracking_copy = global_state
            .tracking_copy(root_hash)
            .expect("Obtaining root hash succeed")
            .expect("Root hash exists");

        let transaction_hash_bytes: [u8; 32] = [1; 32];
        let transaction_hash: TransactionHash =
            TransactionHash::V1(TransactionV1Hash::from_raw(transaction_hash_bytes));
        let id = Id::Transaction(transaction_hash);
        let address_generator = Arc::new(RwLock::new(AddressGenerator::new(
            &id.seed(),
            Phase::Session,
        )));

        let runtime_native_config = RuntimeNativeConfig::default();

        //
        // Mint source purse
        //

        let ret = dispatch_system_contract(
            &mut tracking_copy,
            runtime_native_config.clone(),
            transaction_hash,
            Arc::clone(&address_generator),
            |mut runtime| runtime.mint(U512::from(1000u64)),
        );

        let source_uref = ret.expect("dispatch mint").expect("uref");

        //
        // Mint dest purse
        //

        let ret = dispatch_system_contract(
            &mut tracking_copy,
            runtime_native_config.clone(),
            transaction_hash,
            Arc::clone(&address_generator),
            |mut runtime| runtime.mint(U512::from(0u64)),
        );

        let dest_purse = ret.expect("dispatch mint").expect("uref");

        //
        // Check source balance
        //

        let ret: Result<Result<U512, _>, _> = dispatch_system_contract(
            &mut tracking_copy,
            runtime_native_config.clone(),
            transaction_hash,
            Arc::clone(&address_generator),
            |mut runtime| runtime.total_balance(source_uref),
        );

        assert_eq!(ret.unwrap(), Ok(U512::from(1000u64)));

        //
        // Transfer from source to dest
        //
        let ret: Result<Result<(), _>, _> = dispatch_system_contract(
            &mut tracking_copy,
            runtime_native_config,
            transaction_hash,
            Arc::clone(&address_generator),
            |mut runtime| {
                runtime.transfer(None, source_uref, dest_purse, U512::from(1000u64), None)
            },
        );

        assert_eq!(ret.unwrap(), Ok(()));

        let post_root_hash = global_state
            .commit_effects(root_hash, tracking_copy.effects())
            .expect("Should apply effect");

        assert_ne!(post_root_hash, root_hash);
    }
}
