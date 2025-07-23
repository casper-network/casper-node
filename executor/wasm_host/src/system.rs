//! System contract wire up for the new engine.
//!
//! This module wraps system contract logic into a dispatcher that can be used by the new engine
//! hiding the complexity of the underlying implementation.
use std::{cell::RefCell, rc::Rc, sync::Arc};

use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::{InternalHostError, VMError, VMResult};
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
    account::AccountHash, system::SystemEntityType, CLValueError, ContextAccessRights, EntityAddr,
    Key, Phase, PublicKey, SystemHashRegistry, TransactionHash, URef, METHOD_TRANSFER, U512,
};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, error};

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
    system_contract: SystemEntityType,
    func: impl FnOnce(RuntimeNative<R>) -> Ret,
) -> Result<Ret, DispatchError> {
    let system_entity_registry = {
        let stored_value = tracking_copy
            .read(&Key::SystemEntityRegistry)
            .map_err(DispatchError::Storage)?
            .ok_or(DispatchError::RegistryNotFound)?;
        stored_value
            .into_cl_value()
            .expect("should convert stored value into CLValue")
            .into_t::<SystemHashRegistry>()
            .map_err(DispatchError::CLValue)?
    };
    let system_entity_name = system_contract.entity_name();
    let system_entity_addr = system_entity_registry
        .get(&system_entity_name)
        .ok_or(DispatchError::MissingSystemContract(system_entity_name))?;
    let entity_addr = EntityAddr::new_system(*system_entity_addr);

    let runtime_footprint = tracking_copy
        .runtime_footprint_by_entity_addr(entity_addr)
        .map_err(DispatchError::RuntimeFootprint)?;

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
        SystemEntityType::Mint,
        |mut runtime| runtime.mint(U512::zero()),
    ) {
        Ok(mint_result) => mint_result,
        Err(error) => {
            error!(%error, "create purse failed on dispatch");
            return Err(VMError::Internal(InternalHostError::DispatchSystemContract));
        }
    };

    match mint_result {
        Ok(uref) => Ok(uref),
        Err(casper_types::system::mint::Error::GasLimit) => Err(VMError::OutOfGas),
        Err(mint_error) => {
            error!(%mint_error, "create purse failed with error");
            Err(VMError::Internal(InternalHostError::DispatchSystemContract))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MintTransferArgs {
    maybe_to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
}

impl MintTransferArgs {
    pub fn new_simple(source: URef, target: URef, amount: U512) -> Self {
        MintTransferArgs {
            source,
            target,
            amount,
            maybe_to: None,
            id: None,
        }
    }
}

pub fn transfer<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    runtime_native_config: RuntimeNativeConfig,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: MintTransferArgs,
) -> Result<(), DispatchError> {
    let transfer_result: Result<(), casper_types::system::mint::Error> =
        match dispatch_system_contract(
            tracking_copy,
            runtime_native_config,
            id,
            address_generator,
            SystemEntityType::Mint,
            |mut runtime| {
                let MintTransferArgs {
                    maybe_to,
                    source,
                    target,
                    amount,
                    id,
                } = args;

                runtime.transfer(maybe_to, source, target, amount, id)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                error!(%error, "transfer failed on dispatch");
                return Err(DispatchError::Internal(
                    InternalHostError::DispatchSystemContract,
                ));
            }
        };

    debug!(?args, ?transfer_result, METHOD_TRANSFER);

    match transfer_result {
        Ok(()) => Ok(()),
        Err(casper_types::system::mint::Error::InsufficientFunds) => {
            Err(DispatchError::Call(CallError::CalleeReverted))
        }
        Err(casper_types::system::mint::Error::GasLimit) => {
            Err(DispatchError::Call(CallError::CalleeGasDepleted))
        }
        Err(mint_error) => {
            error!(%mint_error, ?args, "transfer failed with error");
            Err(DispatchError::Internal(
                InternalHostError::DispatchSystemContract,
            ))
        }
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
        system::SystemEntityType, ChainspecRegistry, Digest, GenesisConfig, Phase, ProtocolVersion,
        StorageCosts, SystemConfig, Timestamp, TransactionHash, TransactionV1Hash, WasmConfig,
        U512,
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
            true,
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

        let ret = dispatch_system_contract(
            &mut tracking_copy,
            runtime_native_config.clone(),
            transaction_hash,
            Arc::clone(&address_generator),
            SystemEntityType::Mint,
            |mut runtime| runtime.mint(U512::from(1000u64)),
        );

        let uref = ret.expect("dispatch mint").expect("uref");

        let ret: Result<Result<U512, _>, _> = dispatch_system_contract(
            &mut tracking_copy,
            runtime_native_config,
            transaction_hash,
            Arc::clone(&address_generator),
            SystemEntityType::Mint,
            |mut runtime| runtime.total_balance(uref),
        );

        assert_eq!(ret.unwrap(), Ok(U512::from(1000u64)));

        let post_root_hash = global_state
            .commit_effects(root_hash, tracking_copy.effects())
            .expect("Should apply effect");

        assert_ne!(post_root_hash, root_hash);
    }
}
