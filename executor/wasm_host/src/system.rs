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
use casper_executor_wasm_interface::{GasUsage, InternalHostError};
use casper_storage::{
    global_state::GlobalStateReader,
    system::runtime_native::{Id, RuntimeNative},
    tracking_copy::TrackingCopyError,
    AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{
    bytesrepr, ApiError, CLValueError, Phase, PublicKey, TransactionHash, URef, U512,
};
use parking_lot::RwLock;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use thiserror::Error;
use tracing::{debug, error};

use casper_executor_wasm_interface::executor::{
    AuctionMethods, ExecuteError, ExecuteResult, MintMethods, SystemMenu,
};
use casper_types::bytesrepr::ToBytes;

use crate::system;
use casper_types::system::auction::{DelegatorKind, Reservation};

pub use activate_bid::{activate_bid, ActivateBidArgs};
pub use add_bid::{add_bid, AddBidArgs};
pub use add_reservations::{add_reservations, AddReservationsArgs};
pub use burn::{burn, BurnArgs};
pub use cancel_reservations::{cancel_reservations, CancelReservationsArgs};
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
    Internal(InternalHostError),
    #[error("Call error: {0}")]
    Call(CallError),
    #[error("Api error: {0}")]
    Api(ApiError),
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
            DispatchError::Internal(InternalHostError::DispatchSystemContract)
        })?;

        func(runtime)
    };

    // SAFETY: `RuntimeNative` is dropped in the block above, we can extract the tracking copy the
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
    gas_usage: GasUsage, // unfortunately, ExecuteResult needs this value so we tunnel it
    input: Bytes,
    system_menu_selection: SystemMenu,
) -> Result<ExecuteResult, ExecuteError> {
    let ret: Result<Option<Bytes>, DispatchError> = match system_menu_selection {
        SystemMenu::Auction(method) => match method {
            AuctionMethods::Activate => {
                let unpacked: (PublicKey,) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args =
                    ActivateBidArgs::new(unpacked.0, runtime_native_config.minimum_bid_amount());
                system::activate_bid(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                )
                .map(|_| None)
            }
            AuctionMethods::Bid => {
                let unpacked: (PublicKey, u8, U512, u64, u64, u64, u32, u32) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = AddBidArgs::new(
                    unpacked.0, unpacked.1, unpacked.2, unpacked.3, unpacked.4, unpacked.5,
                    unpacked.6, unpacked.7,
                );
                match system::add_bid(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                ) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Withdraw => {
                let unpacked: (PublicKey, U512, u64) = bytesrepr::deserialize_from_slice(&input)
                    .map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = WithdrawBidArgs::new(unpacked.0, unpacked.1, unpacked.2);
                match system::withdraw_bid(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                ) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Delegate => {
                let unpacked: (DelegatorKind, PublicKey, U512, u32) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = DelegateArgs::new(unpacked.0, unpacked.1, unpacked.2, unpacked.3);
                match system::delegate(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                ) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Undelegate => {
                let unpacked: (DelegatorKind, PublicKey, U512) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = UndelegateArgs::new(unpacked.0, unpacked.1, unpacked.2);

                match system::undelegate(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                ) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::Redelegate => {
                let unpacked: (DelegatorKind, PublicKey, U512, PublicKey) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = RedelegateArgs::new(unpacked.0, unpacked.1, unpacked.2, unpacked.3);

                match system::redelegate(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                ) {
                    Ok(ret) => match ret.to_bytes() {
                        Ok(ret_bytes) => Ok(Some(Bytes::from(ret_bytes))),
                        Err(_) => Err(DispatchError::Api(ApiError::Formatting)),
                    },
                    Err(err) => Err(err),
                }
            }
            AuctionMethods::AddReservation => {
                let unpacked: (Vec<Reservation>,) = bytesrepr::deserialize_from_slice(&input)
                    .map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = AddReservationsArgs::new(unpacked.0);

                system::add_reservations(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                )
                .map(|_| None)
            }
            AuctionMethods::CancelReservation => {
                let unpacked: (PublicKey, Vec<DelegatorKind>, u32) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = CancelReservationsArgs::new(unpacked.0, unpacked.1, unpacked.2);

                system::cancel_reservations(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                )
                .map(|_| None)
            }
            AuctionMethods::ChangePublicKey => {
                let unpacked: (PublicKey, PublicKey) = bytesrepr::deserialize_from_slice(&input)
                    .map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = ChangeBidPublicKeyArgs::new(unpacked.0, unpacked.1);

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
        SystemMenu::Mint(method) => match method {
            MintMethods::Burn => {
                let unpacked: (URef, U512) =
                    bytesrepr::deserialize_from_slice(&input).map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = BurnArgs::new(unpacked.0, unpacked.1);
                system::burn(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                )
                .map(|_| None)
            }
            MintMethods::Transfer => {
                let unpacked: (URef, URef, U512) = bytesrepr::deserialize_from_slice(&input)
                    .map_err(|_err| {
                        ExecuteError::InternalHost(InternalHostError::TypeConversion)
                    })?;
                let args = TransferArgs::new(unpacked.0, unpacked.1, unpacked.2);
                system::transfer(
                    &mut tracking_copy,
                    runtime_native_config,
                    transaction_hash,
                    Arc::clone(&address_generator),
                    args,
                )
                .map(|_| None)
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
                    Some(ExecuteError::InternalHost(
                        InternalHostError::DispatchSystemContract,
                    )),
                )
            }
            DispatchError::MissingSystemContract(name) => {
                error!(?transaction_hash, ?name, "system contract not found");
                (
                    None,
                    None,
                    Some(ExecuteError::InternalHost(
                        InternalHostError::DispatchSystemContract,
                    )),
                )
            }
            DispatchError::Internal(ihe) => {
                error!(?transaction_hash, %ihe, "internal host error");
                (None, None, Some(ExecuteError::InternalHost(ihe)))
            }
            DispatchError::Storage(tce) | DispatchError::RuntimeFootprint(tce) => {
                error!(?transaction_hash, %tce, "tracking copy error");
                (
                    None,
                    None,
                    Some(ExecuteError::InternalHost(InternalHostError::TrackingCopy)),
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
