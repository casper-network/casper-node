//! Implementation of all host functions.
pub(crate) mod abi;

use bytes::Bytes;
use casper_storage::tracking_copy::{self, TrackingCopyExt};
use casper_types::{
    account::AccountHash,
    addressable_entity::NamedKeyAddr,
    contracts::{ContractManifest, ContractV2, EntryPointV2},
    ByteCode, ByteCodeAddr, ByteCodeKind, Digest, EntityAddr, EntityVersions, Groups, Key, Package,
    PackageStatus, StoredValue, URef, U512,
};
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
        runtime::{self, MintArgs},
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
        Some(keyspace) => keyspace_to_global_state_key(caller.context().state_address, keyspace),
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    let value = caller.memory_read(value_ptr, value_size.try_into().unwrap())?;

    caller
        .context_mut()
        .storage
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
    let global_state_key = keyspace_to_global_state_key(caller.context().state_address, keyspace);

    match caller.context_mut().storage.read(&global_state_key) {
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

fn keyspace_to_global_state_key<'a>(address: [u8; 32], keyspace: Keyspace<'a>) -> Key {
    match keyspace {
        Keyspace::State => casper_types::Key::State(EntityAddr::SmartContract(address)),
        Keyspace::Context(payload) => {
            let digest = Digest::hash(payload);
            casper_types::Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                EntityAddr::SmartContract(address),
                digest.value(),
            ))
        }
    }
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

    let entrypoints = {
        let mut vec = Vec::new();

        for entry_point in entry_points {
            vec.push(storage::EntryPoint {
                selector: entry_point.selector,
                function_index: entry_point.fptr,
                flags: EntryPointFlags::from_bits_truncate(entry_point.flags),
            })
        }

        vec
    };

    // 1. Store package hash
    let access_key = URef::default();

    let contract_package = Package::new(
        access_key,
        EntityVersions::new(),
        BTreeSet::new(),
        Groups::new(),
        PackageStatus::Unlocked,
    );

    let mut rng = rand::thread_rng(); // TODO: Deterministic random number generator
    let package_hash: Address = rng.gen(); // TODO: Do we need packages at all in new VM?
    let contract_hash: Address = rng.gen();

    caller.context_mut().storage.write(
        Key::Package(package_hash),
        StoredValue::Package(contract_package),
    );

    // 2. Store wasm
    let wasm_hash = Digest::hash(&code);

    let bytecode = ByteCode::new(ByteCodeKind::V1CasperWasm, code.clone().into());
    let bytecode_addr = ByteCodeAddr::V1CasperWasm(wasm_hash.value());

    caller.context_mut().storage.write(
        Key::ByteCode(bytecode_addr),
        StoredValue::ByteCode(bytecode),
    );

    // 3. Store addressable entity

    let key = Key::AddressableEntity(EntityAddr::SmartContract(contract_hash));

    // let owner = EntityAddr::SmartContract(caller.context().address);

    // TODO: abort(str) as an alternative to trap
    let address_generator = Arc::clone(&caller.context().address_generator);
    let transaction_hash = caller.context().transaction_hash;
    let purse_uref: URef = match runtime::mint_mint(
        &mut caller.context_mut().storage,
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

    let entry_points = entrypoints
        .iter()
        .map(|entrypoint| {
            let entry_point = EntryPointV2 {
                selector: entrypoint.selector,
                function_index: entrypoint.function_index,
                flags: entrypoint.flags.bits(),
            };
            entry_point
        })
        .collect();

    let contract = ContractV2::V2(ContractManifest {
        owner: caller.context().caller,
        bytecode_addr,
        entry_points,
        purse_uref,
    });

    caller
        .context_mut()
        .storage
        .write(key, StoredValue::ContractV2(contract));

    let initial_state = match constructor_selector {
        Some(constructor_selector) => {
            // Find all entrypoints with a flag set to CONSTRUCTOR
            let constructors: Vec<_> = entrypoints
                .iter()
                .filter_map(|entrypoint| {
                    if entrypoint.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                        Some(entrypoint)
                    } else {
                        None
                    }
                })
                .filter(|entry_point| entry_point.selector == constructor_selector.get())
                .collect();

            // TODO: Should we validate amount of constructors or just rely on the fact that the proc
            // macro will statically check it, and the document the behavior that only first
            // constructor will be called

            match constructors.first() {
                Some(first_constructor) => {
                    // Take the gas spent so far and use it as a limit for the new VM.
                    let gas_limit = caller
                        .gas_consumed()
                        .try_into_remaining()
                        .expect("should be remaining");

                    let execute_request = ExecuteRequestBuilder::default()
                        .with_caller(caller.context().caller)
                        .with_gas_limit(gas_limit)
                        .with_target(ExecutionKind::Contract {
                            address: contract_hash,
                            selector: Selector::new(selector),
                        })
                        .with_input(input_data.unwrap_or_default())
                        .with_value(value)
                        .with_transaction_hash(caller.context().transaction_hash)
                        // We're using shared address generator there as we need to preserve and advance the state of deterministic address generator across chain of calls.
                        .with_shared_address_generator(Arc::clone(
                            &caller.context().address_generator,
                        ))
                        .build()
                        .expect("should build");

                    let tracking_copy_for_ctor = caller.context().storage.fork2();

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
                                .storage
                                .merge_raw_parts(tracking_copy_parts);

                            output
                        }
                        Err(ExecuteError::Preparation(preparation_error)) => {
                            // This is a bug in the EE, as it should have been caught during the preparation phase when the contract was stored in the global state.
                            todo!()
                        }
                    }
                }
                None => {
                    // No constructor found
                    todo!("No constructor found")
                }
            }
        }
        None => {
            // No constructor selector provided, assuming no initial state
            // There may be an "implicit" state that will be created as part of a write operation to a `Keyspace::State`.
            None
        }
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
            .storage
            .write(key, StoredValue::RawBytes(state.into()));
    }

    let create_result = CreateResult {
        package_address: package_hash,
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

    let tracking_copy = caller.context().storage.fork2();

    // Take the gas spent so far and use it as a limit for the new VM.
    let gas_limit = caller
        .gas_consumed()
        .try_into_remaining()
        .expect("should be remaining");

    let execute_request = ExecuteRequestBuilder::default()
        .with_caller(caller.context().caller)
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
                        .storage
                        .merge_raw_parts(tracking_copy_parts);
                    Ok(())
                }
            };

            (gas_usage, host_result)
        }
        Err(ExecuteError::Preparation(preparation_error)) => {
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

    let data = if path == &[CASPER_ENV_CALLER] {
        Some(caller.context().caller.to_vec())
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
) -> VMResult<u32> {
    let mut data = caller.context().caller.as_slice();
    if dest_ptr == 0 {
        Ok(dest_ptr)
    } else {
        let dest_len = dest_len as usize;
        data = &data[0..cmp::min(32, dest_len as usize)];
        caller.memory_write(dest_ptr, &data)?;
        Ok(dest_ptr + (data.len() as u32))
    }
}

pub(crate) fn casper_env_value<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<S, E>,
) -> u64 {
    caller.context().value
}

const CASPER_ENV_BALANCE_ACCOUNT: u32 = 0;
const CASPER_ENV_BALANCE_CONTRACT: u32 = 1;

pub(crate) fn casper_env_balance<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<S, E>,
    entity_kind: u32,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
) -> VMResult<u64> {
    let entity_key = match entity_kind {
        CASPER_ENV_BALANCE_ACCOUNT => {
            if entity_addr_len != 32 {
                return Ok(0);
            }
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().unwrap());

            let account_key = Key::Account(account_hash);
            match caller.context_mut().storage.read(&account_key) {
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
        CASPER_ENV_BALANCE_CONTRACT => {
            if entity_addr_len != 32 {
                return Ok(0);
            }
            let hash_bytes = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            Key::AddressableEntity(EntityAddr::SmartContract(hash_bytes.try_into().unwrap()))
        }
        _ => return Ok(0),
    };

    let purse = match caller.context_mut().storage.read(&entity_key) {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
            addressable_entity.main_purse()
        }
        Ok(Some(StoredValue::ContractV2(contract_v2))) => {
            // TODO: Remove once 2.0 integration is done with v2 support.
            match contract_v2 {
                ContractV2::V2(contract) => contract.purse_uref,
                _ => panic!("Expected ContractV2::V2"),
            }
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
        .storage
        .get_total_balance(Key::URef(purse))
        .expect("Total balance");
    assert!(total_balance.value() <= U512::from(u64::MAX));
    let total_balance: u64 = total_balance.value().as_u64();
    Ok(total_balance)
}
