use std::{cell::RefCell, rc::Rc, sync::Arc};

use casper_storage::{
    data_access_layer::mint,
    system::{
        self,
        mint::Mint,
        runtime_native::{Config, Id, RuntimeNative},
    },
    tracking_copy::{TrackingCopyExt, TrackingCopyParts},
    AddressGenerator,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{NamedKeyAddr, NamedKeys},
    execution::Effects,
    AddressableEntity, AddressableEntityHash, ContextAccessRights, EntityAddr, Key, Phase,
    ProtocolVersion, PublicKey, SystemEntityRegistry, URef, U512,
};
use parking_lot::RwLock;

use crate::{
    executor::{ExecuteError, ExecuteRequest, ExecuteResult, ExecutionKind, Executor},
    wasm_backend::{Caller, PreparationError},
};

use super::{Address, GlobalStateReader, TrackingCopy};

fn dispatch_system_contract<R: GlobalStateReader, Ret>(
    tracking_copy: &mut TrackingCopy<R>,
    id: Id,
    system_contract: &str,
    func: impl FnOnce(RuntimeNative<R>) -> Ret,
) -> Ret {
    let system_entity_registry = {
        let stored_value = tracking_copy
            .read(&Key::SystemEntityRegistry)
            .expect("should read system entity registry")
            .expect("should get system entity registry");
        stored_value
            .into_cl_value()
            .expect("should convert stored value into CLValue")
            .into_t::<SystemEntityRegistry>()
            .expect("should get system entity registry")
    };
    let system_entity_addr = system_entity_registry
        .get(system_contract)
        .expect("should get mint");
    let entity_addr = EntityAddr::new_system(system_entity_addr.value());
    let addressable_entity = tracking_copy
        .read(&Key::AddressableEntity(entity_addr))
        .expect("should read addressable entity")
        .expect("should get addressable entity")
        .into_addressable_entity()
        .expect("should convert stored value into addressable entity");

    let config = Config::default();
    let protocol_version = ProtocolVersion::V1_0_0;

    let access_rights = ContextAccessRights::new(*system_entity_addr, []);
    let address = PublicKey::System.to_account_hash();

    let named_keys = tracking_copy
        .get_named_keys(entity_addr)
        .expect("should get named keys");

    let forked_tracking_copy = Rc::new(RefCell::new(tracking_copy.fork2()));

    let remaining_spending_limit = U512::MAX; // NOTE: Since there's no custom payment, there's no need to track the remaining spending limit.
    let phase = Phase::System; // NOTE: Since this is a system contract, the phase is always `System`.

    let ret = {
        let runtime = RuntimeNative::new(
            config,
            protocol_version,
            id,
            Rc::clone(&forked_tracking_copy),
            address,
            addressable_entity,
            named_keys,
            access_rights,
            remaining_spending_limit,
            phase,
        );

        func(runtime)
    };

    // SAFETY: `RuntimeNative` is dropped in the block above, we can extract the tracking copy and the effects.
    let modified_tracking_copy = Rc::try_unwrap(forked_tracking_copy)
        .ok()
        .expect("should have no other references");
    let modified_tracking_copy = modified_tracking_copy.into_inner();

    tracking_copy.merge_raw_parts(modified_tracking_copy.into_raw_parts());

    ret
}

/// A trait that provides a simplified interface to the runtime native to execute system contracts.
#[derive(Clone, Debug)]
struct NativeExecutor;

const TEST_MINT_ADDRESS: Address = [111u8; 32];

impl Executor for NativeExecutor {
    fn execute<R: GlobalStateReader + 'static>(
        &self,
        mut tracking_copy: TrackingCopy<R>,
        execute_request: ExecuteRequest,
    ) -> Result<ExecuteResult, ExecuteError> {
        let system_entity_registry = {
            let stored_value = tracking_copy
                .read(&Key::SystemEntityRegistry)
                .expect("Should read system entity registry")
                .expect("should get system entity registry");
            stored_value
                .into_cl_value()
                .expect("should convert stored value into CLValue")
                .into_t::<SystemEntityRegistry>()
                .expect("should get system entity registry")
        };

        match execute_request.execution_kind {
            ExecutionKind::WasmBytes(_) => {
                // Code paths should not lead here, this is intended to be reachable from the Wasm only through `casper_call` host function.
                panic!("WasmBytes execution kind is not supported");
            }
            ExecutionKind::Contract { address, selector } => {
                // This is just a placeholder to show how the system contract can be dispatched.
                // In reality, we can't check the address like this, but rather compare the entry points, no wasm bytes, etc.
                if address == TEST_MINT_ADDRESS {
                    todo!()
                } else {
                    return Err(ExecuteError::Preparation(PreparationError::Instantiation(
                        "Unable to instantiate native runtime".to_string(),
                    )));
                }
            }
        }

        // let runtime_native = {
        //     let entity_addr = self.entity_addr;
        //     let config = Config::default();
        //     let protocol_version = ProtocolVersion::V1_0_0;
        //     let id = Id::Seed(vec![1, 2, 3]);
        //     let tracking_copy = tracking_copy;
        //     let addressable_entity = self.addressable_entity.clone();
        //     let access_rights = ContextAccessRights::new(self.addressable_entity_hash, []);
        //     let tracking_copy = Rc::new(RefCell::new(tracking_copy));
        //     let remaining_spending_limit = U512::MAX; // NOTE: Since there's no custom payment, there's no need to track the remaining spending limit.
        //     let phase = casper_types::Phase::System;
        //     let named_keys = NamedKeys::new();
        //     let address = PublicKey::System.to_account_hash();
        //     let runtime_native = casper_storage::system::runtime_native::RuntimeNative::new(
        //         config,
        //         protocol_version,
        //         id,
        //         tracking_copy,
        //         address,
        //         addressable_entity,
        //         named_keys,
        //         access_rights,
        //         remaining_spending_limit,
        //         phase,
        //     );
        //     RuntimeNative {
        //         entity_addr,
        //         runtime_native,
        //     }
        // };
    }
}

pub(crate) fn mint_mint<R: GlobalStateReader>(
    tracking_copy: &mut TrackingCopy<R>,
    initial_value: impl Into<U512>,
) -> Result<URef, casper_types::system::mint::Error> {
    let id: Id = Id::Seed(vec![1, 2, 3]);
    let initial_value: U512 = initial_value.into();
    dispatch_system_contract(tracking_copy, id, "mint", |mut runtime| {
        runtime.mint(initial_value)
    })
}

#[cfg(test)]
mod tests {
    use casper_storage::{
        data_access_layer::{GenesisRequest, GenesisResult},
        global_state::{
            self,
            state::{CommitProvider, StateProvider},
        },
        system::mint::{storage_provider::StorageProvider, Mint},
    };
    use casper_types::{
        AddressableEntityHash, ChainspecRegistry, Digest, GenesisConfigBuilder, HoldsEpoch,
        SystemEntityRegistry,
    };

    use super::*;

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

        let ret = dispatch_system_contract(
            &mut tracking_copy,
            Id::Seed(vec![1, 2, 3]),
            "mint",
            |mut runtime| runtime.mint(U512::from(1000u64)),
        );

        let uref = ret.expect("Mint");

        let ret = dispatch_system_contract(
            &mut tracking_copy,
            Id::Seed(vec![1, 2, 3]),
            "mint",
            |mut runtime| runtime.total_balance(uref),
        );

        assert_eq!(ret, Ok(U512::from(1000u64)));

        let post_root_hash = global_state
            .commit(root_hash, tracking_copy.effects())
            .expect("Should apply effect");

        assert_ne!(post_root_hash, root_hash);
    }

    // #[test]
    // fn test_mint() {
    //     let (global_state, mut root_hash, _tempdir) =
    //         global_state::state::lmdb::make_temporary_global_state([]);

    //     let genesis_config = GenesisConfigBuilder::default().build();

    //     let genesis_request: GenesisRequest = GenesisRequest::new(
    //         Digest::hash("foo"),
    //         ProtocolVersion::V2_0_0,
    //         genesis_config,
    //         ChainspecRegistry::new_with_genesis(b"", b""),
    //     );

    //     match global_state.genesis(genesis_request) {
    //         GenesisResult::Failure(failure) => panic!("Failed to run genesis: {:?}", failure),
    //         GenesisResult::Fatal(fatal) => panic!("Fatal error while running genesis: {}", fatal),
    //         GenesisResult::Success {
    //             post_state_hash,
    //             effects: _,
    //         } => {
    //             root_hash = post_state_hash;
    //         }
    //     }

    //     let mut tracking_copy = global_state
    //         .tracking_copy(root_hash)
    //         .expect("Obtaining root hash succeed")
    //         .expect("Root hash exists");

    //     let system_entity_registry = {
    //         let stored_value = tracking_copy
    //             .read(&Key::SystemEntityRegistry)
    //             .expect("should read system entity registry")
    //             .expect("should get system entity registry");
    //         stored_value
    //             .into_cl_value()
    //             .expect("should convert stored value into CLValue")
    //             .into_t::<SystemEntityRegistry>()
    //             .expect("should get system entity registry")
    //     };

    //     let mint = system_entity_registry.get("mint").expect("should get mint");
    //     let entity_addr = EntityAddr::new_system(mint.value());
    //     let addressable_entity = tracking_copy
    //         .read(&Key::AddressableEntity(entity_addr))
    //         .expect("should read addressable entity")
    //         .expect("should get addressable entity")
    //         .into_addressable_entity()
    //         .expect("should convert stored value into addressable entity");

    //     let mut runtime = RuntimeNative::new(
    //         entity_addr,
    //         Config::default(),
    //         ProtocolVersion::V1_0_0,
    //         Id::Seed(vec![1, 2, 3]),
    //         tracking_copy,
    //         addressable_entity,
    //         ContextAccessRights::new(AddressableEntityHash::new([5; 32]), []),
    //     );

    //     let source = runtime.mint(U512::from(1000u64)).expect("Should mint");
    //     let source_balance = runtime
    //         .total_balance(source)
    //         .expect("Should get total balance");
    //     assert_eq!(source_balance, U512::from(1000u64));

    //     let target = runtime.mint(U512::from(1u64)).expect("Should create uref");
    //     let target_balance = runtime
    //         .total_balance(target)
    //         .expect("Should get total balance");
    //     assert_eq!(target_balance, U512::from(1u64));

    //     runtime
    //         .transfer(
    //             None,
    //             source,
    //             target,
    //             U512::from(999),
    //             None,
    //             HoldsEpoch::NOT_APPLICABLE,
    //         )
    //         .expect("Should transfer");

    //     let source_balance = runtime
    //         .total_balance(source)
    //         .expect("Should get total balance");
    //     assert_eq!(source_balance, U512::from(1u64));
    //     let target_balance = runtime
    //         .total_balance(target)
    //         .expect("Should get total balance");
    //     assert_eq!(target_balance, U512::from(1000u64));

    //     runtime
    //         .mint_into_existing_purse(target, U512::from(1000u64))
    //         .expect("Should mint");

    //     let target_balance = runtime
    //         .total_balance(target)
    //         .expect("Should get total balance");
    //     assert_eq!(target_balance, U512::from(2000u64));
    // }
}
