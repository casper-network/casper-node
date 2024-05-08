//! Implementation of all host functions.
pub(crate) mod abi;
pub(crate) mod utils;

use bytes::Bytes;
use casper_storage::tracking_copy::{self, TrackingCopyExt};
use casper_types::{
    account::AccountHash,
    addressable_entity::{
        self, ActionThresholds, AssociatedKeys, EntryPointV2, MessageTopics, NamedKeyAddr,
    },
    AddressableEntity, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, Digest, EntityAddr,
    EntityKind, EntryPointAddr, EntryPointValue, Groups, HoldsEpoch, Key, Package, PackageHash,
    PackageStatus, ProtocolVersion, StoredValue, TransactionRuntime, URef, U512,
};
use num_traits::ToBytes;
use rand::Rng;
use safe_transmute::SingleManyGuard;
use std::{cmp, collections::BTreeSet, mem, num::NonZeroU32, sync::Arc};
use tracing::error;
use vm_common::{
    flags::{EntryPointFlags, ReturnFlags},
    keyspace::Keyspace,
    selector::Selector,
};

use crate::{
    executor::{ExecuteError, ExecuteRequestBuilder, ExecuteResult, ExecutionKind},
    host::abi::{CreateResult, EntryPoint, Manifest},
    storage::{
        self,
        runtime::{self, MintArgs, MintTransferArgs},
        Address, GlobalStateReader,
    },
    wasm_backend::{Caller, Context, MeteringPoints, WasmInstance},
    ConfigBuilder, Executor, HostError, HostResult, TrapCode, VMError, VMResult, WasmEngine,
};

use self::abi::ReadInfo;

/// Write value under a key.
pub(crate) fn casper_write<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
    value_ptr: u32,
    value_size: u32,
) -> VMResult<i32> {
    let key = caller.memory_read(key_ptr, key_size.try_into().unwrap())?;

    let global_state_key = match Keyspace::from_raw_parts(key_space, &key) {
        Some(keyspace) => keyspace_to_global_state_key(caller.context(), keyspace),
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    let global_state_key = match global_state_key {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    let value = caller.memory_read(value_ptr, value_size.try_into().unwrap())?;

    caller
        .context_mut()
        .tracking_copy
        .write(global_state_key, StoredValue::RawBytes(value));

    Ok(0)
}

pub(crate) fn casper_print<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<S, E>,
    message_ptr: u32,
    message_size: u32,
) -> VMResult<()> {
    let vec = caller.memory_read(message_ptr, message_size.try_into().unwrap())?;
    let msg = String::from_utf8_lossy(&vec);
    eprintln!("⛓️ {msg}");
    Ok(())
}

/// Write value under a key.
pub(crate) fn casper_read<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    key_tag: u64,
    key_ptr: u32,
    key_size: u32,
    info_ptr: u32,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> Result<i32, VMError> {
    // TODO: Opportunity for optimization: don't read data under key_ptr if given key space does not require it.
    let key = caller.memory_read(key_ptr, key_size.try_into().unwrap())?;

    let keyspace = match Keyspace::from_raw_parts(key_tag, &key) {
        Some(keyspace) => keyspace,
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    // Convert keyspace to a global state `Key`
    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    match caller.context_mut().tracking_copy.read(&global_state_key) {
        Ok(Some(StoredValue::RawBytes(raw_bytes))) => {
            let out_ptr: u32 = if cb_alloc != 0 {
                caller.alloc(cb_alloc, raw_bytes.len(), alloc_ctx)?
            } else {
                // treats alloc_ctx as data
                alloc_ctx
            };

            let read_info = ReadInfo {
                data: out_ptr,
                data_size: raw_bytes.len().try_into().unwrap(),
            };

            let read_info_bytes = safe_transmute::transmute_one_to_bytes(&read_info);
            caller.memory_write(info_ptr, read_info_bytes)?;
            if out_ptr != 0 {
                caller.memory_write(out_ptr, &raw_bytes)?;
            }
            Ok(0)
        }
        Ok(Some(stored_value)) => {
            // TODO: Backwards compatibility with old EE, although it's not clear if we should do it at the storage level.
            // Since new VM has storage isolated from the Wasm (i.e. we have Keyspace on the wasm which gets converted to a global state `Key`).
            // I think if we were to pursue this we'd add a new `Keyspace` enum variant for each old VM supported Key types (i.e. URef, Dictionary perhaps) for some period of time, then deprecate this.
            todo!("Unsupported {stored_value:?}")
        }
        Ok(None) => Ok(1), // Entry does not exists
        Err(error) => {
            // To protect the network against potential non-determinism (i.e. one validator runs out of space or just faces I/O issues that other validators may not have) we're simply aborting the process, hoping that once the node goes back online issues are resolved on the validator side.
            // TODO: We should signal this to the contract runtime somehow, and let validator nodes skip execution.
            error!(?error, "Error while reading from storage; aborting");
            panic!("Error while reading from storage; aborting key={global_state_key:?} error={error:?}")
        }
    }
}

fn keyspace_to_global_state_key<'a, S: GlobalStateReader, E: Executor>(
    context: &Context<S, E>,
    keyspace: Keyspace<'a>,
) -> Option<Key> {
    let entity_addr = match context.callee {
        Key::Account(account_hash) => EntityAddr::new_account(account_hash.value()),
        Key::AddressableEntity(addressable_entity_hash) => {
            EntityAddr::new_smart_contract(addressable_entity_hash.value())
        }
        other => {
            // This should never happen, as the caller is always an account or a smart contract.
            return None;
        }
    };

    let global_state_key = match keyspace {
        Keyspace::State => Key::State(entity_addr),
        Keyspace::Context(payload) => {
            let digest = Digest::hash(payload);
            casper_types::Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                entity_addr,
                digest.value(),
            ))
        }
    };

    Some(global_state_key)
}

pub(crate) fn casper_copy_input<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    let input = caller.config().input.clone();

    let out_ptr: u32 = if cb_alloc != 0 {
        caller.alloc(cb_alloc, input.len(), alloc_ctx)?
    } else {
        // treats alloc_ctx as data
        alloc_ctx
    };

    if out_ptr == 0 {
        Ok(out_ptr)
    } else {
        caller.memory_write(out_ptr, &input)?;
        Ok(out_ptr + (input.len() as u32))
    }
}

/// Returns from the execution of a smart contract with an optional flags.
pub(crate) fn casper_return<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<S, E>,
    flags: u32,
    data_ptr: u32,
    data_len: u32,
) -> VMResult<()> {
    let flags = ReturnFlags::from_bits_retain(flags);
    let data = if data_ptr == 0 {
        None
    } else {
        let data = caller
            .memory_read(data_ptr, data_len.try_into().unwrap())
            .map(Bytes::from)?;
        Some(data)
    };
    Err(VMError::Return { flags, data })
}

pub(crate) fn casper_create<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<S, E>,
    code_ptr: u32,
    code_len: u32,
    manifest_ptr: u32,
    value: u64,
    selector: u32,
    input_ptr: u32,
    input_len: u32,
    result_ptr: u32,
) -> VMResult<HostResult> {
    let code = if code_ptr != 0 {
        caller
            .memory_read(code_ptr, code_len as usize)
            .map(Bytes::from)?
    } else {
        caller.bytecode()
    };

    // For calling a constructor
    let constructor_selector = NonZeroU32::new(selector);

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller.memory_read(input_ptr, input_len as _)?.into();
        Some(input_data)
    };

    let manifest = caller.memory_read(manifest_ptr, mem::size_of::<Manifest>())?;
    let bytes = manifest.as_slice();

    let manifest = match safe_transmute::transmute_one::<Manifest>(bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            todo!("handle error {:?}", error);
        }
    };

    let entry_points_bytes = caller.memory_read(
        manifest.entry_points_ptr,
        (manifest.entry_points_size as usize) * mem::size_of::<EntryPoint>(),
    )?;

    let entry_points =
        match safe_transmute::transmute_many::<EntryPoint, SingleManyGuard>(&entry_points_bytes) {
            Ok(entry_points) => entry_points,
            Err(error) => todo!("handle error {:?}", error),
        };

    // 1. Store package hash
    let contract_package = Package::new(
        Default::default(),
        Default::default(),
        Groups::default(),
        PackageStatus::Unlocked,
    );

    let mut rng = rand::thread_rng(); // TODO: Deterministic random number generator
    let package_hash_bytes: Address = rng.gen(); // TODO: Do we need packages at all in new VM?
    let contract_hash: Address = rng.gen();

    caller.context_mut().tracking_copy.write(
        Key::Package(package_hash_bytes),
        StoredValue::Package(contract_package),
    );

    // 2. Store wasm
    let bytecode_hash = Digest::hash(&code);

    let bytecode = ByteCode::new(ByteCodeKind::V1CasperWasm, code.clone().into());
    let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash.value());

    caller.context_mut().tracking_copy.write(
        Key::ByteCode(bytecode_addr),
        StoredValue::ByteCode(bytecode),
    );

    // 3. Store addressable entity

    let entity_addr = EntityAddr::SmartContract(contract_hash);
    let addressable_entity_key = Key::AddressableEntity(entity_addr);

    // TODO: abort(str) as an alternative to trap
    let address_generator = Arc::clone(&caller.context().address_generator);
    let transaction_hash = caller.context().transaction_hash;
    let main_purse: URef = match runtime::mint_mint(
        &mut caller.context_mut().tracking_copy,
        transaction_hash,
        address_generator,
        MintArgs {
            initial_balance: U512::zero(),
        },
    ) {
        Ok(uref) => uref,
        Err(mint_error) => {
            error!(?mint_error, "Failed to create a purse");
            return Ok(Err(HostError::CalleeTrapped(
                TrapCode::UnreachableCodeReached,
            )));
        }
    };

    for entrypoint in entry_points {
        let key = Key::EntryPoint(EntryPointAddr::VmCasperV2 {
            entity_addr,
            selector: entrypoint.selector,
        });
        let value = EntryPointValue::V2CasperVm(EntryPointV2 {
            function_index: entrypoint.fptr,
            flags: entrypoint.flags,
        });
        let stored_value = StoredValue::EntryPoint(value);
        caller.context_mut().tracking_copy.write(key, stored_value);
    }

    let addressable_entity = AddressableEntity::new(
        PackageHash::new(package_hash_bytes),
        ByteCodeHash::new(bytecode_hash.value()),
        ProtocolVersion::V2_0_0,
        main_purse,
        AssociatedKeys::default(),
        ActionThresholds::default(),
        MessageTopics::default(),
        EntityKind::SmartContract(TransactionRuntime::VmCasperV2),
    );

    caller.context_mut().tracking_copy.write(
        addressable_entity_key,
        StoredValue::AddressableEntity(addressable_entity),
    );

    let initial_state = match constructor_selector {
        Some(_) => {
            // Take the gas spent so far and use it as a limit for the new VM.
            let gas_limit = caller
                .gas_consumed()
                .try_into_remaining()
                .expect("should be remaining");

            let execute_request = ExecuteRequestBuilder::default()
                .with_initiator(caller.context().initiator)
                .with_caller_key(caller.context().callee)
                .with_callee_key(addressable_entity_key)
                .with_gas_limit(gas_limit)
                .with_target(ExecutionKind::Contract {
                    address: contract_hash,
                    selector: Selector::new(selector),
                })
                .with_input(input_data.unwrap_or_default())
                .with_value(value)
                .with_transaction_hash(caller.context().transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the state of deterministic address generator across chain of calls.
                .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
                .build()
                .expect("should build");

            let tracking_copy_for_ctor = caller.context().tracking_copy.fork2();

            match caller
                .context()
                .executor
                .execute(tracking_copy_for_ctor, execute_request)
            {
                Ok(ExecuteResult {
                    host_error,
                    output,
                    gas_usage,
                    tracking_copy_parts,
                }) => {
                    // output

                    caller.consume_gas(gas_usage.gas_spent());

                    if let Some(host_error) = host_error {
                        return Ok(Err(host_error));
                    }

                    caller
                        .context_mut()
                        .tracking_copy
                        .merge_raw_parts(tracking_copy_parts);

                    output
                }
                Err(ExecuteError::WasmPreparation(preparation_error)) => {
                    // This is a bug in the EE, as it should have been caught during the preparation phase when the contract was stored in the global state.
                    todo!()
                }
            }
        }
        None => None,
    };

    if let Some(state) = initial_state {
        eprintln!(
            "Write initial state {}bytes under {contract_hash:?}",
            state.len()
        );
        let smart_contract_addr = EntityAddr::SmartContract(contract_hash);
        let key = Key::State(smart_contract_addr);
        caller
            .context_mut()
            .tracking_copy
            .write(key, StoredValue::RawBytes(state.into()));
    }

    let create_result = CreateResult {
        package_address: package_hash_bytes,
        contract_address: contract_hash,
        version: 1,
    };

    let create_result_bytes = safe_transmute::transmute_one_to_bytes(&create_result);

    debug_assert_eq!(
        safe_transmute::transmute_one(create_result_bytes),
        Ok(create_result),
        "Sanity check", // NOTE: Remove these guards with sufficient test coverage
    );

    caller.memory_write(result_ptr, create_result_bytes)?;

    Ok(Ok(()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn casper_call<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<S, E>,
    address_ptr: u32,
    address_len: u32,
    value: u64,
    selector: u32,
    input_ptr: u32,
    input_len: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<HostResult> {
    // 1. Look up address in the storage
    // 1a. if it's legacy contract, wire up old EE, pretend you're 1.x. Input data would be
    // "RuntimeArgs". Serialized output of the call has to be passed as output. Value is ignored as
    // you can't pass value (tokens) to called contracts. 1b. if it's new contract, wire up
    // another VM as according to the bytecode format. 2. Depends on the VM used (old or new) at
    // this point either entry point is validated (i.e. EE returned error) or will be validated as
    // for now. 3. If entry point is valid, call it, transfer the value, pass the input data. If
    // it's invalid, return error. 4. Output data is captured by calling `cb_alloc`.
    // let vm = VM::new();
    // vm.
    let address = caller.memory_read(address_ptr, address_len as _)?;
    let address: Address = address.try_into().unwrap(); // TODO: Error handling

    let input_data: Bytes = caller.memory_read(input_ptr, input_len as _)?.into();

    let tracking_copy = caller.context().tracking_copy.fork2();

    // Take the gas spent so far and use it as a limit for the new VM.
    let gas_limit = caller
        .gas_consumed()
        .try_into_remaining()
        .expect("should be remaining");

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_callee_key(Key::AddressableEntity(EntityAddr::new_smart_contract(
            address,
        )))
        .with_gas_limit(gas_limit)
        .with_target(ExecutionKind::Contract {
            address,
            selector: Selector::new(selector),
        })
        .with_value(value)
        .with_input(input_data)
        .with_transaction_hash(caller.context().transaction_hash)
        // We're using shared address generator there as we need to preserve and advance the state of deterministic address generator across chain of calls.
        .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
        .build()
        .expect("should build");

    let (gas_usage, host_result) = match caller
        .context()
        .executor
        .execute(tracking_copy, execute_request)
    {
        Ok(ExecuteResult {
            host_error,
            output,
            gas_usage,
            tracking_copy_parts,
        }) => {
            if let Some(output) = output {
                let out_ptr: u32 = if cb_alloc != 0 {
                    caller.alloc(cb_alloc, output.len(), cb_ctx)?
                } else {
                    // treats alloc_ctx as data
                    cb_ctx
                };

                if out_ptr != 0 {
                    caller.memory_write(out_ptr, &output)?;
                }
            }

            let host_result = match host_error {
                Some(host_error) => Err(host_error),
                None => {
                    caller
                        .context_mut()
                        .tracking_copy
                        .merge_raw_parts(tracking_copy_parts);
                    Ok(())
                }
            };

            (gas_usage, host_result)
        }
        Err(ExecuteError::WasmPreparation(preparation_error)) => {
            // This is a bug in the EE, as it should have been caught during the preparation phase when the contract was stored in the global state.
            unreachable!("Preparation error: {:?}", preparation_error)
        }
    };

    let gas_spent = gas_usage
        .gas_limit
        .checked_sub(gas_usage.remaining_points)
        .expect("remaining points always below or equal to the limit");

    match caller.consume_gas(gas_spent) {
        MeteringPoints::Remaining(_) => {}
        MeteringPoints::Exhausted => {
            todo!("exhausted")
        }
    }

    Ok(host_result)
}

const CASPER_ENV_CALLER: u64 = 0;

pub(crate) fn casper_env_read<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    env_path: u32,
    env_path_length: u32,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    // NOTE: Needs versioning to disallow situtation where a path contains values that may be used
    // in the future.

    let size: usize = env_path_length.try_into().unwrap();
    let env_path_bytes = caller.memory_read(env_path, size * 8)?;

    let path = match safe_transmute::transmute_many::<u64, SingleManyGuard>(&env_path_bytes) {
        Ok(env_path) => env_path,
        Err(safe_transmute::Error::Unaligned(unaligned_error))
            if unaligned_error.source.is_empty() =>
        {
            &[]
        }
        Err(error) => todo!("handle error {:?}", error),
    };

    let data: Option<Address> = if path == &[CASPER_ENV_CALLER] {
        // Some(caller.context().caller.to_vec())
        todo!("Not really using this one")
    } else {
        None
    };

    if let Some(data) = data {
        let out_ptr: u32 = if cb_alloc != 0 {
            caller.alloc(cb_alloc, data.len(), alloc_ctx)?
        } else {
            // treats alloc_ctx as data
            alloc_ctx
        };

        if out_ptr == 0 {
            Ok(out_ptr)
        } else {
            caller.memory_write(out_ptr, &data)?;
            Ok(out_ptr + (data.len() as u32))
        }
    } else {
        Ok(0)
    }
}

pub(crate) fn casper_env_caller<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    dest_ptr: u32,
    dest_len: u32,
    entity_kind_ptr: u32,
) -> VMResult<u32> {
    // TODO: Decide whether we want to return the full address and entity kind or just the 32 bytes "unified".
    let (entity_kind, data) = match &caller.context().caller {
        Key::Account(account_hash) => (0u32, account_hash.value()),
        Key::AddressableEntity(entity_addr) => (1u32, entity_addr.value()),
        other => panic!("Unexpected caller: {:?}", other),
    };
    let mut data = &data[..];
    if dest_ptr == 0 {
        Ok(dest_ptr)
    } else {
        let dest_len = dest_len as usize;
        data = &data[0..cmp::min(32, dest_len as usize)];
        caller.memory_write(dest_ptr, &data)?;
        let entity_kind_bytes = entity_kind.to_le_bytes();
        caller.memory_write(entity_kind_ptr, entity_kind_bytes.as_slice())?;
        Ok(dest_ptr + (data.len() as u32))
    }
}

pub(crate) fn casper_env_value<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<S, E>,
) -> u64 {
    caller.context().value
}

const CASPER_ENTITY_ACCOUNT: u32 = 0;
const CASPER_ENTITY_CONTRACT: u32 = 1;

pub(crate) fn casper_env_balance<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    entity_kind: u32,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
) -> VMResult<u64> {
    let entity_key = match entity_kind {
        CASPER_ENTITY_ACCOUNT => {
            if entity_addr_len != 32 {
                return Ok(0);
            }
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().unwrap());

            let account_key = Key::Account(account_hash);
            match caller.context_mut().tracking_copy.read(&account_key) {
                Ok(Some(StoredValue::CLValue(clvalue))) => {
                    let entity_key = clvalue.into_t::<Key>().expect("should be a key");
                    entity_key
                }
                Ok(Some(other_entity)) => {
                    panic!("Unexpected entity type: {:?}", other_entity)
                }
                Ok(None) => return Ok(0),
                Err(error) => panic!("Error while reading from storage; aborting key={account_key:?} error={error:?}"),
                _ => return Ok(0),
            }
        }
        CASPER_ENTITY_CONTRACT => {
            if entity_addr_len != 32 {
                return Ok(0);
            }
            let hash_bytes = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            Key::AddressableEntity(EntityAddr::SmartContract(hash_bytes.try_into().unwrap()))
        }
        _ => return Ok(0),
    };

    let purse = match caller.context_mut().tracking_copy.read(&entity_key) {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
            addressable_entity.main_purse()
        }
        Ok(Some(other_entity)) => {
            panic!("Unexpected entity type: {:?}", other_entity)
        }
        Ok(None) => return Ok(0),
        Err(error) => {
            panic!("Error while reading from storage; aborting key={entity_key:?} error={error:?}")
        }
    };

    let total_balance = caller
        .context_mut()
        .tracking_copy
        .get_total_balance(Key::URef(purse))
        .expect("Total balance");
    assert!(total_balance.value() <= U512::from(u64::MAX));
    let total_balance: u64 = total_balance.value().as_u64();
    Ok(total_balance)
}

pub(crate) fn casper_transfer<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    entity_kind: u32,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
    amount: u64,
) -> VMResult<u32> {
    if entity_addr_len != 32 {
        assert!(false, "Invalid target address length");
        return Ok(0); // fail
    }

    let entity_key = match entity_kind {
        CASPER_ENTITY_ACCOUNT => {
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().unwrap());

            // Resolve indirection to get to the actual addressable entity
            match caller
                .context_mut()
                .tracking_copy
                .read(&Key::Account(account_hash))
                .expect("should read account")
            {
                Some(StoredValue::CLValue(indirect)) => {
                    // is it an account?
                    let addressable_entity_key = indirect.into_t::<Key>().expect("should be key");
                    addressable_entity_key
                }
                Some(other) => panic!("should be cl value but got {other:?}"),
                None => panic!("Expected account to exist"),
            }
        }
        CASPER_ENTITY_CONTRACT => {
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let entity_addr: Address = entity_addr.try_into().unwrap();
            let entity_key = Key::AddressableEntity(EntityAddr::SmartContract(entity_addr));
            todo!("Find an payable entrypoint to call {entity_key:?}");
        }
        _ => return Ok(0),
    };

    let callee_purse = match caller.context().callee {
        account_key @ Key::Account(_account_hash) => {
            match caller
                .context_mut()
                .tracking_copy
                .read(&account_key)
                .expect("should read account")
            {
                Some(StoredValue::CLValue(indirect)) => {
                    // is it an account?
                    let key = indirect.into_t::<Key>().expect("should be key");
                    let stored_value = caller
                        .context_mut()
                        .tracking_copy
                        .read(&key)
                        .expect("should read account")
                        .expect("should have account");
                    let addressable_entity = stored_value
                        .into_addressable_entity()
                        .expect("should be addressable entity");
                    addressable_entity.main_purse()
                }
                Some(other) => panic!("should be cl value but got {other:?}"),
                None => panic!("Expected account to exist"),
            }
        }
        addressable_entity_key @ Key::AddressableEntity(_entity_addr) => {
            match caller
                .context_mut()
                .tracking_copy
                .read(&addressable_entity_key)
                .expect("should read account")
            {
                Some(StoredValue::AddressableEntity(addressable_entity)) => {
                    addressable_entity.main_purse()
                }
                Some(other) => panic!("should be addressable entity but got {other:?}"),
                None => todo!(),
            }
        }
        other => panic!("should be account or addressable entity but got {other:?}"),
    };

    let target_purse = match caller.context_mut().tracking_copy.read(&entity_key) {
        Ok(Some(StoredValue::Account(account))) => {
            panic!("Expected AddressableEntity but got {:?}", account)
        }
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
            addressable_entity.main_purse()
        }
        Ok(Some(other_entity)) => {
            panic!("Unexpected entity type: {:?}", other_entity)
        }
        Ok(None) => {
            panic!("Addressable entity not found for key={entity_key:?}");
        }
        Err(error) => {
            panic!("Error while reading from storage; aborting key={entity_key:?} error={error:?}")
        }
    };

    let transaction_hash = caller.context().transaction_hash;
    let address_generator = Arc::clone(&caller.context().address_generator);
    let args = MintTransferArgs {
        source: callee_purse,
        target: target_purse,
        amount: U512::from(amount),
        maybe_to: None,
        id: None,
    };

    runtime::mint_transfer(
        &mut caller.context_mut().tracking_copy,
        transaction_hash,
        address_generator,
        args,
    )
    .expect("Should transfer");

    Ok(1)
}
