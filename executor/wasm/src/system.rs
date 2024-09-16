//! System contract wire up for the new engine.
//!
//! This module wraps system contract logic into a dispatcher that can be used by the new engine
//! hiding the complexity of the underlying implementation.
use std::{cell::RefCell, rc::Rc, sync::Arc};

use casper_executor_wasm_interface::{HostError, HostResult, TrapCode};
use casper_storage::{
    global_state::GlobalStateReader,
    system::{
        mint::Mint,
        runtime_native::{Config, Id, RuntimeNative},
    },
    tracking_copy::{TrackingCopyError, TrackingCopyExt},
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, MessageTopics},
    AddressableEntity, ByteCodeHash, CLValueError, ContextAccessRights, Contract, EntityAddr,
    EntityKind, Key, PackageHash, Phase, ProtocolVersion, PublicKey, StoredValue, StoredValueTag,
    SystemEntityRegistry, TransactionHash, TransactionRuntime, URef, U512,
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
    #[error("Missing system contract: {0}")]
    MissingSystemContract(&'static str),
    #[error("Error getting named keys")]
    GetNamedKeys(TrackingCopyError),
    #[error("Invalid key variant")]
    InvalidStoredValueVariant {
        expected: StoredValueTag,
        actual: StoredValue,
    },
}

fn dispatch_system_contract<R: GlobalStateReader, Ret: PartialEq>(
    tracking_copy: &mut TrackingCopy<R>,
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
            .into_t::<SystemEntityRegistry>()
            .map_err(DispatchError::CLValue)?
    };
    let system_entity_addr = system_entity_registry
        .get(system_contract)
        .ok_or(DispatchError::MissingSystemContract(system_contract))?;
    let entity_addr = EntityAddr::new_system(system_entity_addr.value());

    // let addressable_entity_stored_value =

    let addressable_entity = match tracking_copy.read(&Key::AddressableEntity(entity_addr))? {
        Some(StoredValue::AddressableEntity(addressable_entity)) => addressable_entity,
        Some(addressable_entity_stored_value) => {
            return Err(DispatchError::InvalidStoredValueVariant {
                expected: StoredValueTag::AddressableEntity,
                actual: addressable_entity_stored_value,
            })
        }
        None => {
            let legacy_contract_stored_value = tracking_copy
                .read(&Key::Hash(system_entity_addr.value()))?
                .expect("Legacy contract structure should be present");
            let legacy_contract: Contract = legacy_contract_stored_value
                .into_contract()
                .expect("should convert stored value into contract");
            AddressableEntity::new(
                PackageHash::new(legacy_contract.contract_package_hash().value()), //: PackageHash,
                ByteCodeHash::new(legacy_contract.contract_wasm_hash().value()), //: ByteCodeHash,
                legacy_contract.protocol_version(), //: ProtocolVersion,
                URef::default(),                    // main_purse
                AssociatedKeys::default(),          //: AssociatedKeys,
                ActionThresholds::default(),        //: ActionThresholds,
                MessageTopics::default(),
                EntityKind::SmartContract(TransactionRuntime::VmCasperV1),
            )
        }
    };

    let config = Config::default();
    let protocol_version = ProtocolVersion::V1_0_0;

    let access_rights = ContextAccessRights::new(*system_entity_addr, []);
    let address = PublicKey::System.to_account_hash();

    let named_keys = tracking_copy
        .get_named_keys(entity_addr)
        .map_err(DispatchError::GetNamedKeys)?;

    let forked_tracking_copy = Rc::new(RefCell::new(tracking_copy.fork2()));

    let remaining_spending_limit = U512::MAX; // NOTE: Since there's no custom payment, there's no need to track the remaining spending limit.
    let phase = Phase::System; // NOTE: Since this is a system contract, the phase is always `System`.

    let ret = {
        let runtime = RuntimeNative::new(
            config,
            protocol_version,
            Id::Transaction(transaction_hash),
            address_generator,
            Rc::clone(&forked_tracking_copy),
            address,
            Key::AddressableEntity(entity_addr),
            addressable_entity,
            named_keys,
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

    tracking_copy
        .commit(modified_tracking_copy.effects())
        .expect("Should commit effects");

    Ok(ret)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MintArgs {
    pub(crate) initial_balance: U512,
}

pub(crate) fn mint_mint<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    transaction_hash: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: MintArgs,
) -> Result<URef, HostError> {
    let mint_result = match dispatch_system_contract(
        tracking_copy,
        transaction_hash,
        address_generator,
        "mint",
        |mut runtime| runtime.mint(args.initial_balance),
    ) {
        Ok(mint_result) => mint_result,
        Err(error) => {
            error!(%error, ?args, "mint failed");
            panic!("Mint failed with error {error:?}; aborting");
        }
    };

    match mint_result {
        Ok(uref) => Ok(uref),
        Err(casper_types::system::mint::Error::InsufficientFunds) => Err(HostError::CalleeReverted),
        Err(casper_types::system::mint::Error::GasLimit) => Err(HostError::CalleeGasDepleted),
        Err(mint_error) => {
            error!(%mint_error, ?args, "mint transfer failed");
            Err(HostError::CalleeTrapped(TrapCode::UnreachableCodeReached))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct MintTransferArgs {
    pub(crate) maybe_to: Option<AccountHash>,
    pub(crate) source: URef,
    pub(crate) target: URef,
    pub(crate) amount: U512,
    pub(crate) id: Option<u64>,
}

pub(crate) fn mint_transfer<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    id: TransactionHash,
    address_generator: Arc<RwLock<AddressGenerator>>,
    args: MintTransferArgs,
) -> HostResult {
    let transfer_result: Result<(), casper_types::system::mint::Error> =
        match dispatch_system_contract(
            tracking_copy,
            id,
            address_generator,
            "mint",
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
                error!(%error, "mint transfer failed");
                return Err(HostError::CalleeTrapped(TrapCode::UnreachableCodeReached));
            }
        };

    debug!(?args, ?transfer_result, "transfer");

    match transfer_result {
        Ok(()) => Ok(()),
        Err(casper_types::system::mint::Error::InsufficientFunds) => Err(HostError::CalleeReverted),
        Err(casper_types::system::mint::Error::GasLimit) => Err(HostError::CalleeGasDepleted),
        Err(mint_error) => {
            error!(%mint_error, ?args, "mint transfer failed");
            Err(HostError::CalleeTrapped(TrapCode::UnreachableCodeReached))
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
        AddressGenerator,
    };
    use casper_types::{
        ChainspecRegistry, Digest, GenesisConfigBuilder, Phase, ProtocolVersion, TransactionHash,
        TransactionV1Hash, U512,
    };
    use parking_lot::RwLock;

    use crate::system::dispatch_system_contract;

    #[test]
    fn test_system_dispatcher() {
        let (global_state, mut root_hash, _tempdir) =
            global_state::state::lmdb::make_temporary_global_state([]);

        let genesis_config = GenesisConfigBuilder::default().build();

        let genesis_request: GenesisRequest = GenesisRequest::new(
            Digest::hash("foo"),
            ProtocolVersion::V2_0_0,
            genesis_config,
            ChainspecRegistry::new_with_genesis(b"", b""),
        );

        match global_state.genesis(genesis_request) {
            GenesisResult::Failure(failure) => panic!("Failed to run genesis: {:?}", failure),
            GenesisResult::Fatal(fatal) => panic!("Fatal error while running genesis: {}", fatal),
            GenesisResult::Success {
                post_state_hash,
                effects: _,
            } => {
                root_hash = post_state_hash;
            }
        }

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

        let ret = dispatch_system_contract(
            &mut tracking_copy,
            transaction_hash,
            Arc::clone(&address_generator),
            "mint",
            |mut runtime| runtime.mint(U512::from(1000u64)),
        );

        let uref = ret.expect("dispatch mint").expect("uref");

        let ret: Result<Result<U512, _>, _> = dispatch_system_contract(
            &mut tracking_copy,
            transaction_hash,
            Arc::clone(&address_generator),
            "mint",
            |mut runtime| runtime.total_balance(uref),
        );

        // let ret = ret.expect("dispatch total balance");

        assert_eq!(ret.unwrap(), Ok(U512::from(1000u64)));

        let post_root_hash = global_state
            .commit(root_hash, tracking_copy.effects())
            .expect("Should apply effect");

        assert_ne!(post_root_hash, root_hash);
    }
}
