//! System contract wire up for the new engine.
//!
//! This module wraps system contract logic into a dispatcher that can be used by the new engine
//! hiding the complexity of the underlying implementation.

mod activate_bid;
mod add_bid;
mod add_reservations;
mod cancel_reservations;
mod change_bid_public_key;
mod create_purse;
mod delegate;
mod redelegate;
mod transfer;
mod undelegate;
mod withdraw_bid;

use std::{cell::RefCell, rc::Rc, sync::Arc};

use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::InternalHostError;
use casper_storage::{
    global_state::GlobalStateReader,
    system::runtime_native::{Id, RuntimeNative},
    tracking_copy::TrackingCopyError,
    AddressGenerator, RuntimeNativeConfig, TrackingCopy,
};
use casper_types::{CLValueError, Phase, TransactionHash};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::error;

pub use activate_bid::{activate_bid, ActivateBidArgs};
pub use add_bid::{add_bid, AddBidArgs};
pub use add_reservations::{add_reservations, AddReservationsArgs};
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
    #[error("Internal host error: {0}")]
    Internal(InternalHostError),
    #[error("Call error: {0}")]
    Call(CallError),
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
