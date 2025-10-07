pub(crate) mod altbn128;
use std::{borrow::Cow, collections::BTreeMap, num::NonZeroU32, sync::Arc};

use bytes::Bytes;
use casper_executor_wasm_common::{
    chain_utils,
    entry_point::{
        ENTRY_POINT_PAYMENT_CALLER, ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY,
        ENTRY_POINT_PAYMENT_SELF_ONWARD,
    },
    error::{
        CallError, CALLEE_NOT_CALLABLE, CALLEE_SUCCEEDED, CALLEE_TRAPPED, HOST_ERROR_INVALID_DATA,
        HOST_ERROR_INVALID_INPUT, HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED,
        HOST_ERROR_MESSAGE_TOPIC_FULL, HOST_ERROR_NOT_FOUND, HOST_ERROR_PAYLOAD_TOO_LONG,
        HOST_ERROR_SUCCESS, HOST_ERROR_TOO_MANY_TOPICS, HOST_ERROR_TOPIC_TOO_LONG,
    },
    flags::ReturnFlags,
    keyspace::{Keyspace, KeyspaceTag},
};
use casper_executor_wasm_interface::{
    executor::{
        CryptoMethods, ExecuteError, ExecuteRequestBuilder, ExecuteResult, ExecutionKind, Executor,
    },
    u32_from_host_result, Caller, InternalHostError, VMError, VMResult,
};
use casper_storage::{global_state::GlobalStateReader, tracking_copy::TrackingCopyExt};
use casper_types::{
    account::AccountHash,
    addressable_entity::{
        ActionThresholds, AssociatedKeys, MessageTopicError, NamedKeyAddr, NamedKeyValue,
    },
    bytesrepr::{FromBytes, ToBytes},
    contract_messages::{Message, MessageAddr, MessagePayload, MessageTopicSummary},
    execution::RetValue,
    AccessRights, AddressableEntity, BlockGlobalAddr, BlockHash, BlockTime, ByteCode, ByteCodeAddr,
    ByteCodeHash, ByteCodeKind, CLType, CLValue, Contract, ContractRuntimeTag, ContractWasmHash,
    Digest, EntityAddr, EntityKind, EntryPointPayment, EntryPointValue, HashAddr, HashAlgorithm,
    HostFunctionV2, Key, NamedKeys, Package, PackageHash, ProtocolVersion, Signature, StoredValue,
    URef,
};
use either::Either;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use tracing::{error, info, warn};

use crate::{
    abi::{CreateResult, EnvInfo, ReadInfo},
    context::Context,
    system,
};
use blake2::{
    digest::{Update, VariableOutput},
    Blake2bVar,
};
use casper_executor_wasm_common::{
    chain_utils::{compute_next_contract_hash_version, compute_wasm_bytecode_hash},
    error::{HOST_ERROR_CL_VALUE, HOST_LOCKED_PACKAGE, HOST_NO_ACTIVE_CONTRACT},
};
use casper_executor_wasm_interface::executor::{
    AuctionMethods, ExecuteRequest, MintMethods, SystemMenu,
};
use casper_types::contracts::{ContractHash, ContractPackage, ContractPackageHash, EntryPoints};
use keccak_asm::Digest as KeccakDigest;
use sha2::Sha256;

const NAME_FOR_V2_CONTRACT_MAIN_PURSE: &str = "__main_purse";

#[derive(Debug, Copy, Clone, FromPrimitive, PartialEq)]
enum EntityKindTag {
    Account = 0,
    Contract = 1,
}

pub trait FallibleInto<T> {
    fn wrapped_try_into(self) -> VMResult<T>;
}

impl<From, To> FallibleInto<To> for From
where
    To: TryFrom<From>,
{
    fn wrapped_try_into(self) -> VMResult<To> {
        To::try_from(self).map_err(|_| VMError::Internal(InternalHostError::TypeConversion))
    }
}

/// Consumes imputed amount of gas.
fn charge_gas<S: GlobalStateReader, E: Executor>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    imputed: u64,
) -> VMResult<()> {
    caller.consume_gas(imputed)?;
    Ok(())
}

/// Consumes a set amount of gas for the specified storage value.
fn charge_gas_storage<S: GlobalStateReader, E: Executor>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    size_bytes: usize,
) -> VMResult<()> {
    let storage_costs = &caller.context().storage_costs;
    let gas_cost = storage_costs.calculate_gas_cost(size_bytes);
    let value: u64 = gas_cost.value().try_into().map_err(|_| VMError::OutOfGas)?;
    caller.consume_gas(value)?;
    Ok(())
}

/// Consumes a set amount of gas for the specified host function and weights
fn charge_host_function_call<S, E, const N: usize>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    host_function: &HostFunctionV2<[u64; N]>,
    weights: [u64; N],
) -> VMResult<()>
where
    S: GlobalStateReader,
    E: Executor,
{
    let Some(cost) = host_function.calculate_gas_cost(weights) else {
        // Overflowing gas calculation means gas limit was exceeded
        return Err(VMError::OutOfGas);
    };

    caller.consume_gas(cost.value().as_u64())?;
    Ok(())
}

/// Writes a message to the global state and charges for storage used.
fn metered_write<S: GlobalStateReader, E: Executor>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    key: Key,
    value: StoredValue,
) -> VMResult<()> {
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    charge_gas_storage(caller, value.serialized_length())?;
    caller.context_mut().tracking_copy.write(key, value);
    Ok(())
}

/// Write value under a key.
pub fn casper_write<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
    value_ptr: u32,
    value_size: u32,
) -> VMResult<u32> {
    // In restricted mode, writing is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let write_cost = caller.context().config.host_function_costs().write;
    charge_host_function_call(
        &mut caller,
        &write_cost,
        [
            key_space,
            u64::from(key_ptr),
            u64::from(key_size),
            u64::from(value_ptr),
            u64::from(value_size),
        ],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let key_payload_bytes =
        caller.memory_read(key_ptr.wrapped_try_into()?, key_size.wrapped_try_into()?)?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    // TODO: Invalid key name encoding
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::AllNamedKeys => Keyspace::AllNamedKeys,
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let value = caller.memory_read(
        value_ptr.wrapped_try_into()?,
        value_size.wrapped_try_into()?,
    )?;

    let stored_value = match keyspace {
        Keyspace::State | Keyspace::Context(_) => {
            let cl_value_any = CLValue::from_components(CLType::Any, value);
            StoredValue::CLValue(cl_value_any)
        }
        Keyspace::NamedKey(name) => {
            // NamedKey points to a URef which holds CLValue::Any bytes
            let maybe_stored_value = caller
                .context_mut()
                .tracking_copy
                .read(&global_state_key)
                .map_err(|_| InternalHostError::TrackingCopy)?;

            let stored_value = match maybe_stored_value {
                Some(StoredValue::NamedKey(existing_named_key)) => {
                    let uref_to_use =
                        if let Ok(Key::URef(existing_uref)) = existing_named_key.get_key() {
                            existing_uref
                        } else {
                            let mut address_generator = caller.context().address_generator.write();
                            address_generator.new_uref(AccessRights::NONE)
                        };

                    // Point the named key to the URef
                    let named_key = Key::URef(uref_to_use);
                    let key_name = name.to_string();
                    let Ok(named_key_value) =
                        NamedKeyValue::from_concrete_values(named_key, key_name)
                    else {
                        return Ok(HOST_ERROR_INVALID_DATA);
                    };

                    StoredValue::NamedKey(named_key_value)
                }
                Some(StoredValue::Contract(mut contract)) => {
                    let uref = match contract.named_keys().get(name) {
                        Some(Key::URef(uref)) => *uref,
                        Some(_) => return Ok(HOST_ERROR_INVALID_INPUT),
                        None => {
                            let mut address_generator = caller.context().address_generator.write();
                            address_generator.new_uref(AccessRights::NONE)
                        }
                    };

                    // Write payload bytes under the URef as CLValue::Any
                    let cl_value_any = CLValue::from_components(CLType::Any, value.clone());
                    metered_write(
                        &mut caller,
                        Key::URef(uref),
                        StoredValue::CLValue(cl_value_any),
                    )?;

                    let named_keys = {
                        let mut ret = BTreeMap::new();
                        ret.insert(name.to_string(), Key::URef(uref));
                        NamedKeys::from(ret)
                    };
                    contract.named_keys_append(named_keys);

                    StoredValue::Contract(contract)
                }
                Some(_) => return Ok(HOST_ERROR_NOT_FOUND),
                None => {
                    let uref = {
                        let mut address_generator = caller.context().address_generator.write();
                        address_generator.new_uref(AccessRights::NONE)
                    };
                    // Write payload bytes under the URef as CLValue::Any
                    let cl_value_any = CLValue::from_components(CLType::Any, value.clone());
                    metered_write(
                        &mut caller,
                        Key::URef(uref),
                        StoredValue::CLValue(cl_value_any),
                    )?;

                    // Point the named key to the URef
                    let named_key = Key::URef(uref);
                    let key_name = name.to_string();
                    let Ok(named_key_value) =
                        NamedKeyValue::from_concrete_values(named_key, key_name)
                    else {
                        return Ok(HOST_ERROR_INVALID_DATA);
                    };

                    StoredValue::NamedKey(named_key_value)
                }
            };

            stored_value
        }
        Keyspace::AllNamedKeys => return Ok(HOST_ERROR_INVALID_INPUT),
    };

    metered_write(&mut caller, global_state_key, stored_value)?;

    Ok(HOST_ERROR_SUCCESS)
}

/// Remove value under a key.
///
/// This produces a transformation of Prune to the global state. Keep in mind that technically the
/// data is not removed from the global state as it still there, it's just not reachable anymore
/// from the newly created tip.
///
/// The name for this host function is `remove` to keep it simple and consistent with read/write
/// verbs, and also consistent with the rust stdlib vocabulary i.e. `V`
pub fn casper_remove<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
) -> VMResult<u32> {
    // In restricted mode, removing is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let remove_cost = caller.context().config.host_function_costs().remove;
    charge_host_function_call(
        &mut caller,
        &remove_cost,
        [key_space, u64::from(key_ptr), u64::from(key_size)],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let key_payload_bytes =
        caller.memory_read(key_ptr.wrapped_try_into()?, key_size.wrapped_try_into()?)?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    // TODO: Invalid key name encoding
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::AllNamedKeys => Keyspace::AllNamedKeys,
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let global_state_read_result = caller.context_mut().tracking_copy.read(&global_state_key);
    match global_state_read_result {
        Ok(Some(StoredValue::AddressableEntity(_))) => return Ok(HOST_ERROR_INVALID_INPUT),
        Ok(Some(_)) => {
            // If it's a named key pointing to a URef, prune both the named key and the URef.
            if let Keyspace::NamedKey(_) = keyspace {
                if let Ok(Some(StoredValue::NamedKey(named_key_value))) =
                    caller.context_mut().tracking_copy.read(&global_state_key)
                {
                    if let Ok(Key::URef(uref)) = named_key_value.get_key() {
                        caller.context_mut().tracking_copy.prune(Key::URef(uref));
                    }
                }
            }

            // Produce a prune transform for the named key
            caller.context_mut().tracking_copy.prune(global_state_key);
        }
        Ok(None) => {
            // Entry does not exist, and we can't proceed with the prune operation
            return Ok(HOST_ERROR_NOT_FOUND);
        }
        Err(error) => {
            // To protect the network against potential non-determinism (i.e. one validator runs out
            // of space or just faces I/O issues that other validators may not have) we're simply
            // aborting the process, hoping that once the node goes back online issues are resolved
            // on the validator side. TODO: We should signal this to the contract
            // runtime somehow, and let validator nodes skip execution.
            error!(
                ?error,
                ?global_state_key,
                "Error while attempting a read before removing value; aborting"
            );
            panic!("Error while attempting a read before removing value; aborting key={global_state_key:?} error={error:?}")
        }
    }

    Ok(HOST_ERROR_SUCCESS)
}

pub fn casper_print<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    message_ptr: u32,
    message_size: u32,
) -> VMResult<()> {
    let print_cost = caller.context().config.host_function_costs().print;
    charge_host_function_call(
        &mut caller,
        &print_cost,
        [u64::from(message_ptr), u64::from(message_size)],
    )?;

    let vec = caller.memory_read(
        message_ptr.wrapped_try_into()?,
        message_size.wrapped_try_into()?,
    )?;
    let msg = String::from_utf8_lossy(&vec);
    eprintln!("⛓️ {msg}");
    Ok(())
}

/// Write value under a key.
pub fn casper_read<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_tag: u64,
    key_ptr: u32,
    key_size: u32,
    info_ptr: u32,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    let read_cost = caller.context().config.host_function_costs().read;
    charge_host_function_call(
        &mut caller,
        &read_cost,
        [
            key_tag,
            u64::from(key_ptr),
            u64::from(key_size),
            u64::from(info_ptr),
            u64::from(cb_alloc),
            u64::from(alloc_ctx),
        ],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_tag) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_INVALID_INPUT);
        }
    };

    // TODO: Opportunity for optimization: don't read data under key_ptr if given key space does not
    // require it.
    let key_payload_bytes =
        caller.memory_read(key_ptr.wrapped_try_into()?, key_size.wrapped_try_into()?)?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::AllNamedKeys => Keyspace::AllNamedKeys,
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let global_state_read_result = caller.context_mut().tracking_copy.read(&global_state_key);
    let global_state_raw_bytes: Cow<[u8]> = match global_state_read_result {
        Ok(Some(StoredValue::CLValue(cl_value))) => {
            let CLType::Any = cl_value.cl_type() else {
                return Err(InternalHostError::TypeConversion)?;
            };
            Cow::Owned(cl_value.inner_bytes().to_owned())
        }
        Ok(Some(StoredValue::NamedKey(named_key_value))) => {
            // Dereference named key to its URef and return the underlying Any bytes
            let Ok(Key::URef(uref)) = named_key_value.get_key() else {
                return Ok(HOST_ERROR_INVALID_DATA);
            };

            match caller.context_mut().tracking_copy.read(&Key::URef(uref)) {
                Ok(Some(StoredValue::CLValue(cl_value))) => {
                    let CLType::Any = cl_value.cl_type() else {
                        return Ok(HOST_ERROR_INVALID_DATA);
                    };
                    Cow::Owned(cl_value.inner_bytes().to_owned())
                }
                Ok(Some(_)) => {
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
                Ok(None) => {
                    return Ok(HOST_ERROR_NOT_FOUND);
                }
                Err(_error) => {
                    return Err(InternalHostError::TrackingCopy.into());
                }
            }
        }
        Ok(Some(StoredValue::Contract(contract))) => match keyspace {
            Keyspace::NamedKey(name) => {
                let Some(Key::URef(uref)) = contract.named_keys().get(name) else {
                    return Ok(HOST_ERROR_INVALID_DATA);
                };

                match caller.context_mut().tracking_copy.read(&Key::URef(*uref)) {
                    Ok(Some(StoredValue::CLValue(cl_value))) => {
                        let CLType::Any = cl_value.cl_type() else {
                            return Ok(HOST_ERROR_INVALID_DATA);
                        };
                        Cow::Owned(cl_value.inner_bytes().to_owned())
                    }
                    Ok(Some(_)) => {
                        return Ok(HOST_ERROR_INVALID_DATA);
                    }
                    Ok(None) => {
                        return Ok(HOST_ERROR_NOT_FOUND);
                    }
                    Err(_error) => {
                        return Err(InternalHostError::TrackingCopy.into());
                    }
                }
            }
            Keyspace::AllNamedKeys => match contract.take_named_keys().to_bytes() {
                Ok(bytes) => Cow::Owned(bytes),
                Err(_) => return Ok(HOST_ERROR_INVALID_INPUT),
            },
            _ => {
                error!(?keyspace, "unsupported keyspace");
                return Ok(HOST_ERROR_INVALID_INPUT);
            }
        },
        Ok(Some(StoredValue::AddressableEntity(_))) => {
            if let Keyspace::AllNamedKeys = keyspace {
                let entity_addr = context_to_entity_addr(caller.context());

                let named_keys = caller
                    .context_mut()
                    .tracking_copy
                    .get_named_keys(entity_addr)
                    .map(|named_keys| named_keys.to_bytes());

                match named_keys {
                    Ok(Ok(bytes)) => Cow::Owned(bytes),
                    Ok(_) | Err(_) => return Ok(HOST_ERROR_INVALID_INPUT),
                }
            } else {
                return Ok(HOST_ERROR_INVALID_INPUT);
            }
        }
        Ok(Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point)))) => {
            match entry_point.entry_point_payment() {
                EntryPointPayment::Caller => Cow::Borrowed(&[ENTRY_POINT_PAYMENT_CALLER]),
                EntryPointPayment::DirectInvocationOnly => {
                    Cow::Borrowed(&[ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY])
                }
                EntryPointPayment::SelfOnward => Cow::Borrowed(&[ENTRY_POINT_PAYMENT_SELF_ONWARD]),
            }
        }
        Ok(Some(stored_value)) => {
            // TODO: Backwards compatibility with old EE, although it's not clear if we should
            // do it at the storage level. Since new VM has storage isolated
            // from the Wasm (i.e. we have Keyspace on the wasm which gets
            // converted to a global state `Key`). I think if we were to pursue
            // this we'd add a new `Keyspace` enum variant for each old
            // VM supported Key types (i.e. URef, Dictionary perhaps) for some period of time,
            // then deprecate this.
            todo!("Unsupported {stored_value:?}")
        }
        Ok(None) => return Ok(HOST_ERROR_NOT_FOUND), // Entry does not exist
        Err(error) => {
            // To protect the network against potential non-determinism (i.e. one validator runs
            // out of space or just faces I/O issues that other validators may
            // not have) we're simply aborting the process, hoping that once the
            // node goes back online issues are resolved on the validator side.
            // TODO: We should signal this to the contract runtime somehow, and
            // let validator nodes skip execution.
            error!(?error, "Error while reading from storage; aborting");
            panic!("Error while reading from storage; aborting key={global_state_key:?} error={error:?}")
        }
    };

    let out_ptr: u32 = if cb_alloc != 0 {
        caller.alloc(cb_alloc, global_state_raw_bytes.len(), alloc_ctx)?
    } else {
        // treats alloc_ctx as data
        alloc_ctx
    };

    let read_info = ReadInfo {
        data_ptr: out_ptr,
        data_size: global_state_raw_bytes.len().wrapped_try_into()?,
    };

    let read_info_bytes = borsh::to_vec(&read_info)
        .map_err(|_| VMError::Internal(InternalHostError::Serialization))?;
    caller.memory_write(info_ptr.wrapped_try_into()?, &read_info_bytes)?;
    if out_ptr != 0 {
        caller.memory_write(out_ptr.wrapped_try_into()?, &global_state_raw_bytes)?;
    }
    Ok(HOST_ERROR_SUCCESS)
}

fn keyspace_to_global_state_key<S: GlobalStateReader, E: Executor>(
    context: &Context<S, E>,
    keyspace: Keyspace<'_>,
) -> Option<Key> {
    let entity_addr = context_to_entity_addr(context);
    let ae_enabled = context.tracking_copy.enable_addressable_entity();

    match keyspace {
        Keyspace::State => Some(Key::State(entity_addr)),
        Keyspace::Context(bytes) => {
            let digest = Digest::hash(bytes);
            Some(Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                entity_addr,
                digest.value(),
            )))
        }
        Keyspace::NamedKey(payload) => {
            let digest = Digest::hash(payload.as_bytes());
            Some(Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                entity_addr,
                digest.value(),
            )))
        }
        Keyspace::AllNamedKeys => {
            if ae_enabled {
                Some(Key::AddressableEntity(entity_addr))
            } else {
                match entity_addr {
                    EntityAddr::Account(hash_addr) => {
                        Some(Key::Account(AccountHash::new(hash_addr)))
                    }
                    EntityAddr::SmartContract(hash_addr) => Some(Key::Hash(hash_addr)),
                    _ => None,
                }
            }
        }
    }
}

fn context_to_entity_addr<S: GlobalStateReader, E: Executor>(
    context: &Context<S, E>,
) -> EntityAddr {
    match context.callee {
        Key::Account(account_hash) => EntityAddr::new_account(account_hash.value()),
        Key::Hash(hash_addr) => EntityAddr::SmartContract(hash_addr),
        Key::AddressableEntity(smart_contract_addr) => smart_contract_addr,
        _ => {
            // This should never happen, as the caller is always an account or a smart contract.
            panic!("Unexpected callee variant: {:?}", context.callee)
        }
    }
}

pub fn casper_copy_input<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    let input = caller.context().input.clone();

    let out_ptr: u32 = if cb_alloc != 0 {
        caller.alloc(cb_alloc, input.len(), alloc_ctx)?
    } else {
        // treats alloc_ctx as data
        alloc_ctx
    };

    let copy_input_cost = caller.context().config.host_function_costs().copy_input;
    charge_host_function_call(
        &mut caller,
        &copy_input_cost,
        [
            u64::from(out_ptr),
            input.len().try_into().map_err(|err| {
                error!("Failed to convert u64 to usize. Details: {err}");
                ExecuteError::InternalHost(InternalHostError::TypeConversion)
            })?,
        ],
    )?;

    if out_ptr == 0 {
        Ok(out_ptr)
    } else {
        caller.memory_write(out_ptr.wrapped_try_into()?, &input)?;
        Ok(out_ptr + (input.len() as u32))
    }
}

/// Returns from the execution of a smart contract with an optional flags.
pub fn casper_return<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    flags: u32,
    data_ptr: u32,
    data_len: u32,
) -> VMResult<()> {
    let ret_cost = caller.context().config.host_function_costs().ret;
    charge_host_function_call(
        &mut caller,
        &ret_cost,
        [u64::from(data_ptr), u64::from(data_len)],
    )?;

    let maybe_flags = ReturnFlags::from_bits(flags);
    let flags = match maybe_flags {
        Some(flags) => flags,
        None => {
            return Err(VMError::Execute(ExecuteError::ReturnFlagsNotSupported(
                flags,
            )))
        }
    };
    let data = if data_ptr == 0 {
        None
    } else {
        let data = caller
            .memory_read(data_ptr.wrapped_try_into()?, data_len.wrapped_try_into()?)
            .map(Bytes::from)?;

        let key = caller.context().callee;
        let bytes = casper_types::bytesrepr::Bytes::from(data.to_vec());
        caller
            .context_mut()
            .tracking_copy
            .ret(key, RetValue::Bytes(bytes));

        Some(data)
    };
    Err(VMError::Return { flags, data })
}

#[allow(clippy::too_many_arguments)]
pub fn casper_create<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    code_ptr: u32,
    code_len: u32,
    transferred_value: u64,
    entry_point_ptr: u32,
    entry_point_len: u32,
    input_ptr: u32,
    input_len: u32,
    seed_ptr: u32,
    seed_len: u32,
    result_ptr: u32,
) -> VMResult<u32> {
    // In restricted mode, contract creation is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let create_cost = caller.context().config.host_function_costs().create;
    charge_host_function_call(
        &mut caller,
        &create_cost,
        [
            u64::from(code_ptr),
            u64::from(code_len),
            transferred_value,
            u64::from(entry_point_ptr),
            u64::from(entry_point_len),
            u64::from(input_ptr),
            u64::from(input_len),
            u64::from(seed_ptr),
            u64::from(seed_len),
            u64::from(result_ptr),
        ],
    )?;

    let code = if code_ptr != 0 {
        caller
            .memory_read(code_ptr.wrapped_try_into()?, code_len as usize)
            .map(Bytes::from)?
    } else {
        caller.bytecode()
    };

    let seed = if seed_ptr != 0 {
        if seed_len != 32 {
            return Ok(CALLEE_NOT_CALLABLE);
        }
        let seed_bytes = caller.memory_read(seed_ptr.wrapped_try_into()?, seed_len as usize)?;
        let seed_bytes: [u8; 32] = seed_bytes.try_into().map_err(|_| {
            // SAFETY: We checked for length. This shouldn't happen
            error!("Error when converting seed_bytes from vec to static array");
            ExecuteError::InternalHost(InternalHostError::TypeConversion)
        })?;
        Some(seed_bytes)
    } else {
        None
    };

    // For calling a constructor
    let constructor_entry_point = {
        let entry_point_ptr = NonZeroU32::new(entry_point_ptr);
        match entry_point_ptr {
            Some(entry_point_ptr) => {
                let entry_point_bytes = caller.memory_read(
                    entry_point_ptr.get().wrapped_try_into()?,
                    entry_point_len as _,
                )?;
                match String::from_utf8(entry_point_bytes) {
                    Ok(entry_point) => Some(entry_point),
                    Err(utf8_error) => {
                        error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                        return Ok(CALLEE_NOT_CALLABLE);
                    }
                }
            }
            None => {
                // No constructor to be called
                None
            }
        }
    };

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller
            .memory_read(input_ptr.wrapped_try_into()?, input_len as _)?
            .into();
        Some(input_data)
    };

    let bytecode_hash = chain_utils::compute_wasm_bytecode_hash(&code);

    let bytecode = ByteCode::new(ByteCodeKind::V2CasperWasm, code.clone().into());
    let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash);

    let callee_addr = context_to_entity_addr(caller.context()).value();

    let package_addr: HashAddr = chain_utils::compute_predictable_address(
        caller.context().chain_name.as_bytes(),
        callee_addr,
        bytecode_hash,
        seed,
    );

    let protocol_version = ProtocolVersion::V2_0_0;
    let protocol_version_major = protocol_version.value().major;

    let ae_enabled = caller.context().tracking_copy.enable_addressable_entity();

    let (smart_contract_package_key, smart_contract_package_as_stored_value, smart_contract_addr) =
        if ae_enabled {
            // 1. Store package hash
            let mut smart_contract_package = Package::default();

            let next_version =
                smart_contract_package.next_entity_version_for(protocol_version_major);
            let smart_contract_addr =
                compute_next_contract_hash_version(package_addr, next_version);

            smart_contract_package.insert_entity_version(
                protocol_version_major,
                EntityAddr::SmartContract(smart_contract_addr),
            );

            (
                Key::SmartContract(package_addr),
                StoredValue::SmartContract(smart_contract_package),
                smart_contract_addr,
            )
        } else {
            let mut smart_contract_package = ContractPackage::default();

            let next_version =
                smart_contract_package.next_contract_version_for(protocol_version_major);
            let smart_contract_addr =
                compute_next_contract_hash_version(package_addr, next_version);

            smart_contract_package.insert_contract_version(
                protocol_version_major,
                ContractHash::new(smart_contract_addr),
            );

            (
                Key::Hash(package_addr),
                StoredValue::ContractPackage(smart_contract_package),
                smart_contract_addr,
            )
        };

    if caller
        .context_mut()
        .tracking_copy
        .read(&smart_contract_package_key)
        .map_err(|_| VMError::Internal(InternalHostError::TrackingCopy))?
        .is_some()
    {
        return Err(VMError::Internal(InternalHostError::ContractAlreadyExists));
    }

    metered_write(
        &mut caller,
        smart_contract_package_key,
        smart_contract_package_as_stored_value,
    )?;

    // 2. Store wasm
    if !ae_enabled {
        let byte_code_key = Key::byte_code_key(ByteCodeAddr::V2CasperWasm(bytecode_hash));
        let byte_code_key_as_cl_value = match CLValue::from_t(byte_code_key) {
            Ok(cl_value) => cl_value,
            Err(_) => return Ok(HOST_ERROR_CL_VALUE),
        };

        metered_write(
            &mut caller,
            Key::Hash(bytecode_hash),
            StoredValue::CLValue(byte_code_key_as_cl_value),
        )?
    };

    metered_write(
        &mut caller,
        Key::ByteCode(bytecode_addr),
        StoredValue::ByteCode(bytecode),
    )?;

    // TODO: abort(str) as an alternative to trap
    let address_generator = Arc::clone(&caller.context().address_generator);
    let transaction_hash = caller.context().transaction_hash;
    let runtime_native_config = caller.context().runtime_native_config.clone();
    let main_purse: URef = match system::create_purse(
        &mut caller.context_mut().tracking_copy,
        runtime_native_config,
        transaction_hash,
        address_generator,
    ) {
        Ok(uref) => uref,
        Err(mint_error) => {
            error!(?mint_error, "Failed to create a purse");
            return Ok(CALLEE_TRAPPED);
        }
    };

    if ae_enabled {
        // 3. Store addressable entity
        let entity_addr = EntityAddr::SmartContract(smart_contract_addr);
        let addressable_entity_key = Key::AddressableEntity(entity_addr);

        let addressable_entity = AddressableEntity::new(
            PackageHash::new(package_addr),
            ByteCodeHash::new(bytecode_hash),
            ProtocolVersion::V2_0_0,
            main_purse,
            AssociatedKeys::default(),
            ActionThresholds::default(),
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
        );

        metered_write(
            &mut caller,
            addressable_entity_key,
            StoredValue::AddressableEntity(addressable_entity),
        )?;
    } else {
        let contract_package_hash = ContractPackageHash::new(package_addr);
        let contract_wasm_hash = ContractWasmHash::new(bytecode_hash);

        let named_keys = {
            let mut ret = NamedKeys::default();
            ret.insert(
                NAME_FOR_V2_CONTRACT_MAIN_PURSE.to_string(),
                Key::URef(main_purse),
            );
            ret
        };

        let contract = Contract::new(
            contract_package_hash,
            contract_wasm_hash,
            // TODO: Populate this correctly
            named_keys,
            EntryPoints::default(),
            ProtocolVersion::V2_0_0,
        );

        metered_write(
            &mut caller,
            Key::Hash(smart_contract_addr),
            StoredValue::Contract(contract),
        )?;
    }

    let _initial_state = match constructor_entry_point {
        Some(entry_point_name) => {
            // Limit the new VM to remaining gas.
            let gas_limit = caller
                .get_remaining_points()?
                .try_into_remaining()
                .map_err(|_| InternalHostError::TypeConversion)?;

            let execute_request = ExecuteRequestBuilder::default()
                .with_initiator(caller.context().initiator)
                .with_caller_key(caller.context().callee)
                .with_gas_limit(gas_limit)
                .with_execution_kind(ExecutionKind::Stored {
                    address: package_addr,
                    entry_point: entry_point_name.clone(),
                })
                .with_input(input_data.unwrap_or_default())
                .with_transferred_value(transferred_value)
                .with_transaction_hash(caller.context().transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the
                // state of deterministic address generator across chain of calls.
                .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
                .with_chain_name(caller.context().chain_name.clone())
                .with_block_time(caller.context().block_time)
                .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
                .with_block_height(1) // TODO: Carry on block height
                .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
                .with_runtime_native_config(caller.context().runtime_native_config.clone())
                .build()
                .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

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
                    effects,
                    cache,
                    messages,
                }) => {
                    // output
                    caller.consume_gas(gas_usage.gas_spent())?;

                    if let Some(host_error) = host_error {
                        return Ok(host_error.into_u32());
                    }

                    caller
                        .context_mut()
                        .tracking_copy
                        .apply_changes(effects, cache, messages);

                    output
                }
                Err(execute_error) => {
                    // This is a bug in the EE, as it should have been caught during the preparation
                    // phase when the contract was stored in the global state.
                    error!(?execute_error, "Failed to execute constructor entry point");
                    return Err(VMError::Execute(execute_error));
                }
            }
        }
        None => None,
    };

    let create_result = CreateResult {
        package_address: package_addr,
    };

    let create_result_bytes =
        borsh::to_vec(&create_result).map_err(|_| InternalHostError::Serialization)?;

    caller.memory_write(result_ptr.wrapped_try_into()?, &create_result_bytes)?;

    Ok(CALLEE_SUCCEEDED)
}

#[allow(clippy::too_many_arguments)]
pub fn casper_system<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    system_contract_opt: u32,
    input_ptr: u32,
    input_len: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<u32> {
    // In restricted mode, contract calls are not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }
    // get option so we can determine cost, or charge if invalid
    let option: SystemMenu = match TryFrom::try_from(system_contract_opt) {
        Ok(option) => option,
        Err(_) => {
            // the following can produce a VMError::OutOfGas error
            let penalty_cost = caller.context().baseline_motes_amount;
            charge_gas(&mut caller, penalty_cost)?;
            return Err(InternalHostError::InvalidSystemOption(system_contract_opt).into());
        }
    };

    let cost = match &option {
        SystemMenu::Mint(mint_opt) => match mint_opt {
            MintMethods::Burn => caller.context().mint_costs.burn as u64,
            MintMethods::Transfer | MintMethods::TransferPurse => {
                caller.context().mint_costs.transfer as u64
            }
        },
        SystemMenu::Auction(auction_opt) => match auction_opt {
            AuctionMethods::Activate => caller.context().auction_costs.activate_bid,
            AuctionMethods::Bid => caller.context().auction_costs.add_bid,
            AuctionMethods::Withdraw => caller.context().auction_costs.withdraw_bid,
            AuctionMethods::Delegate => caller.context().auction_costs.delegate,
            AuctionMethods::Undelegate => caller.context().auction_costs.undelegate,
            AuctionMethods::Redelegate => caller.context().auction_costs.redelegate,
            AuctionMethods::AddReservation => caller.context().auction_costs.add_reservations,
            AuctionMethods::CancelReservation => caller.context().auction_costs.cancel_reservations,
            AuctionMethods::ChangePublicKey => caller.context().auction_costs.change_bid_public_key,
        },
        SystemMenu::Crypto(crypto_methods) => {
            let fn_cost = match crypto_methods {
                CryptoMethods::AltBn128Add => {
                    caller.context().config.host_function_costs().alt_bn128_add
                }
                CryptoMethods::AltBn128Multiply => {
                    caller.context().config.host_function_costs().alt_bn128_mul
                }
                CryptoMethods::AltBn128Pairing => {
                    caller
                        .context()
                        .config
                        .host_function_costs()
                        .alt_bn128_pairing
                }
            };
            let Some(cost) =
                fn_cost.calculate_gas_cost([u64::from(input_ptr), u64::from(input_len)])
            else {
                // Overflowing gas calculation means gas limit was exceeded
                return Err(VMError::OutOfGas);
            };
            u64::try_from(cost.value()).map_err(|err| {
                error!("Couldn't execute host function due to cost calculation overflow. Details: {err}");
                VMError::Internal(InternalHostError::TypeConversion)
            })?
        }
    };
    // the following can produce a VMError::OutOfGas error
    charge_gas(&mut caller, cost)?;

    let input_data: Bytes = caller.memory_read(input_ptr, input_len as _)?.into();

    // Limit the call to remaining gas.
    let gas_limit = caller
        .get_remaining_points()?
        .try_into_remaining()
        .map_err(|_| InternalHostError::TypeConversion)?;

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_gas_limit(gas_limit)
        .with_execution_kind(ExecutionKind::System(option))
        .with_input(input_data)
        .with_transaction_hash(caller.context().transaction_hash)
        .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
        .with_chain_name(caller.context().chain_name.clone())
        .with_block_time(caller.context().block_time)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(1) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .with_runtime_native_config(caller.context().runtime_native_config.clone())
        .build()
        .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

    exec(caller, execute_request, cb_alloc, cb_ctx)
}

#[allow(clippy::too_many_arguments)]
pub fn casper_call<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    address_ptr: u32,
    address_len: u32,
    transferred_value: u64,
    entry_point_ptr: u32,
    entry_point_len: u32,
    input_ptr: u32,
    input_len: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<u32> {
    // In restricted mode, contract calls are not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let call_cost = caller.context().config.host_function_costs().call;
    charge_host_function_call(
        &mut caller,
        &call_cost,
        [
            u64::from(address_ptr),
            u64::from(address_len),
            transferred_value,
            u64::from(entry_point_ptr),
            u64::from(entry_point_len),
            u64::from(input_ptr),
            u64::from(input_len),
            u64::from(cb_alloc),
            u64::from(cb_ctx),
        ],
    )?;

    // 1. Look up address in the storage
    // 1a. if it's VM1 contract, wire up old EE, pretend you're 1.x. Input data would be
    // "RuntimeArgs". Serialized output of the call has to be passed as output. Value is ignored as
    // you can't pass value (tokens) to called contracts. 1b. if it's new contract, wire up
    // another VM as according to the bytecode format. 2. Depends on the VM used (old or new) at
    // this point either entry point is validated (i.e. EE returned error) or will be validated as
    // for now. 3. If entry point is valid, call it, transfer the value, pass the input data. If
    // it's invalid, return error. 4. Output data is captured by calling `cb_alloc`.
    // let vm = VM::new();
    // vm.
    let address = caller.memory_read(address_ptr.wrapped_try_into()?, address_len as _)?;
    let smart_contract_addr: HashAddr = address.wrapped_try_into()?;

    let input_data: Bytes = caller
        .memory_read(input_ptr.wrapped_try_into()?, input_len as _)?
        .into();

    let entry_point = {
        let entry_point_bytes =
            caller.memory_read(entry_point_ptr.wrapped_try_into()?, entry_point_len as _)?;
        match String::from_utf8(entry_point_bytes) {
            Ok(entry_point) => entry_point,
            Err(utf8_error) => {
                error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                return Ok(CALLEE_NOT_CALLABLE);
            }
        }
    };

    // Limit the new VM to remaining gas.
    let gas_limit = caller
        .get_remaining_points()?
        .try_into_remaining()
        .map_err(|_| InternalHostError::TypeConversion)?;

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_gas_limit(gas_limit)
        .with_execution_kind(ExecutionKind::Stored {
            address: smart_contract_addr,
            entry_point: entry_point.clone(),
        })
        .with_transferred_value(transferred_value)
        .with_input(input_data)
        .with_transaction_hash(caller.context().transaction_hash)
        // We're using shared address generator there as we need to preserve and advance the state
        // of deterministic address generator across chain of calls.
        .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
        .with_chain_name(caller.context().chain_name.clone())
        .with_block_time(caller.context().block_time)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(1) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .with_runtime_native_config(caller.context().runtime_native_config.clone())
        .build()
        .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

    let ret = exec(caller, execute_request, cb_alloc, cb_ctx);
    if let Err(execute_error) = &ret {
        error!(
            ?execute_error,
            ?smart_contract_addr,
            ?entry_point,
            "Failed to execute entry point"
        );
    }
    ret
}

fn exec<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    execute_request: ExecuteRequest,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<u32> {
    let tracking_copy = caller.context().tracking_copy.fork2();

    let (gas_usage, host_result) = match caller
        .context()
        .executor
        .execute(tracking_copy, execute_request)
    {
        Ok(ExecuteResult {
            host_error,
            output,
            gas_usage,
            effects,
            cache,
            messages,
        }) => {
            if let Some(output) = output {
                let out_ptr: u32 = if cb_alloc != 0 {
                    caller.alloc(cb_alloc, output.len(), cb_ctx)?
                } else {
                    // treats alloc_ctx as data
                    cb_ctx
                };

                if out_ptr != 0 {
                    caller.memory_write(out_ptr.wrapped_try_into()?, &output)?;
                }
            }
            let host_result = match host_error {
                Some(host_error) => Err(host_error),
                None => {
                    caller
                        .context_mut()
                        .tracking_copy
                        .apply_changes(effects, cache, messages);
                    Ok(())
                }
            };

            (gas_usage, host_result)
        }
        Err(execute_error) => {
            return Err(VMError::Execute(execute_error));
        }
    };

    let gas_spent = gas_usage
        .gas_limit()
        .checked_sub(gas_usage.remaining_points())
        .ok_or(InternalHostError::RemainingGasExceedsGasLimit)?;

    caller.consume_gas(gas_spent)?;

    // this will result in the VM being killed
    if let Err(CallError::Api(api_error)) = host_result {
        return Err(VMError::Execute(ExecuteError::Api(api_error)));
    }

    Ok(u32_from_host_result(host_result))
}

pub fn casper_env_balance<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    entity_kind: u32,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
    output_ptr: u32,
) -> VMResult<u32> {
    let balance_cost = caller.context().config.host_function_costs().env_balance;
    charge_host_function_call(
        &mut caller,
        &balance_cost,
        [
            u64::from(entity_kind),
            u64::from(entity_addr_ptr),
            u64::from(entity_addr_len),
            u64::from(output_ptr),
        ],
    )?;

    let entity_key = match EntityKindTag::from_u32(entity_kind) {
        Some(EntityKindTag::Account) => {
            if entity_addr_len != 32 {
                return Ok(HOST_ERROR_SUCCESS);
            }
            let entity_addr = caller.memory_read(
                entity_addr_ptr.wrapped_try_into()?,
                entity_addr_len as usize,
            )?;
            let account_hash: AccountHash = AccountHash::new(entity_addr.wrapped_try_into()?);

            let account_key = Key::Account(account_hash);
            match caller.context_mut().tracking_copy.read(&account_key) {
                Ok(Some(StoredValue::CLValue(clvalue))) => {
                    let addressable_entity_key = clvalue
                        .into_t::<Key>()
                        .map_err(|_| InternalHostError::TypeConversion)?;
                    Either::Right(addressable_entity_key)
                }
                Ok(Some(StoredValue::Account(account))) => Either::Left(account.main_purse()),
                Ok(Some(other_entity)) => {
                    error!("Unexpected entity type: {other_entity:?}");
                    return Err(InternalHostError::UnexpectedEntityKind.into());
                }
                Ok(None) => return Ok(HOST_ERROR_SUCCESS),
                Err(error) => {
                    error!("Error while reading from storage; aborting key={account_key:?} error={error:?}");
                    return Err(InternalHostError::TrackingCopy.into());
                }
            }
        }
        Some(EntityKindTag::Contract) => {
            if entity_addr_len != 32 {
                return Ok(HOST_ERROR_SUCCESS);
            }
            let hash_bytes = caller.memory_read(
                entity_addr_ptr.wrapped_try_into()?,
                entity_addr_len as usize,
            )?;
            let hash_bytes: [u8; 32] = hash_bytes.try_into().map_err(|_| {
                // SAFETY: We checked for length. This shouldn't happen
                error!("Error when converting hash_bytes from vec to static array");
                ExecuteError::InternalHost(InternalHostError::TypeConversion)
            })?;
            let smart_contract_key = if caller.context().tracking_copy.enable_addressable_entity() {
                Key::SmartContract(hash_bytes)
            } else {
                Key::Hash(hash_bytes)
            };
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressable_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressable_entity_hash.value(),
                            ));
                            Either::Right(key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(HOST_ERROR_SUCCESS);
                        }
                    }
                }
                Ok(Some(StoredValue::ContractPackage(contract))) => {
                    match contract.versions().last_key_value() {
                        Some((_, contract_hash)) => Either::Right(Key::Hash(contract_hash.value())),
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(HOST_ERROR_NOT_FOUND);
                        }
                    }
                }
                Ok(Some(_)) => {
                    return Ok(HOST_ERROR_SUCCESS);
                }
                Ok(None) => {
                    // Not found, balance is 0
                    return Ok(HOST_ERROR_SUCCESS);
                }
                Err(error) => {
                    error!(
                        hash_bytes = base16::encode_lower(&hash_bytes),
                        ?error,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        None => return Ok(HOST_ERROR_SUCCESS),
    };

    let purse = match entity_key {
        Either::Left(main_purse) => main_purse,
        Either::Right(indirect_entity_key) => {
            match caller
                .context_mut()
                .tracking_copy
                .read(&indirect_entity_key)
            {
                Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
                    addressable_entity.main_purse()
                }
                Ok(Some(StoredValue::Contract(contract))) => {
                    match contract.named_keys().get(NAME_FOR_V2_CONTRACT_MAIN_PURSE) {
                        Some(Key::URef(uref)) => *uref,
                        None | Some(_) => {
                            // Not found, balance is 0
                            return Ok(HOST_ERROR_SUCCESS);
                        }
                    }
                }
                Ok(Some(other_entity)) => {
                    panic!("Unexpected entity type: {other_entity:?}")
                }
                Ok(None) => panic!("Key not found while checking balance"), //return Ok(0),
                Err(error) => {
                    panic!("Error while reading from storage; aborting key={entity_key:?} error={error:?}")
                }
            }
        }
    };

    let total_balance = caller
        .context_mut()
        .tracking_copy
        .get_total_balance(Key::URef(purse))
        .map_err(|_| InternalHostError::TotalBalanceReadFailure)?;

    let total_balance: u64 = total_balance
        .value()
        .try_into()
        .map_err(|_| InternalHostError::TotalBalanceOverflow)?;

    caller.memory_write(output_ptr.wrapped_try_into()?, &total_balance.to_le_bytes())?;
    Ok(HOST_ERROR_NOT_FOUND)
}

pub fn casper_upgrade<S: GlobalStateReader + 'static, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    code_ptr: u32,
    code_size: u32,
    entry_point_ptr: u32,
    entry_point_size: u32,
    input_ptr: u32,
    input_size: u32,
) -> VMResult<u32> {
    // In restricted mode, contract upgrades are not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let upgrade_cost = caller.context().config.host_function_costs().upgrade;
    charge_host_function_call(
        &mut caller,
        &upgrade_cost,
        [
            u64::from(code_ptr),
            u64::from(code_size),
            u64::from(entry_point_ptr),
            u64::from(entry_point_size),
            u64::from(input_ptr),
            u64::from(input_size),
        ],
    )?;

    let code = caller
        .memory_read(code_ptr.wrapped_try_into()?, code_size as usize)
        .map(Bytes::from)?;

    let entry_point = match NonZeroU32::new(entry_point_ptr) {
        Some(entry_point_ptr) => {
            // There's upgrade entry point to be called
            let entry_point_bytes = caller.memory_read(
                entry_point_ptr.get().wrapped_try_into()?,
                entry_point_size as usize,
            )?;
            match String::from_utf8(entry_point_bytes) {
                Ok(entry_point) => Some(entry_point),
                Err(utf8_error) => {
                    error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                    return Ok(CALLEE_NOT_CALLABLE);
                }
            }
        }
        None => {
            // No constructor to be called
            None
        }
    };

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller
            .memory_read(input_ptr.wrapped_try_into()?, input_size as _)?
            .into();
        Some(input_data)
    };

    let (smart_contract_addr, callee_addressable_entity_key) = match caller.context().callee {
        Key::Account(_account_hash) => {
            error!("Account upgrade is not possible");
            return Ok(CALLEE_NOT_CALLABLE);
        }
        Key::Hash(contract_package_addr) => {
            let smart_contract_package_key = Key::Hash(contract_package_addr);
            match caller
                .context_mut()
                .tracking_copy
                .read(&smart_contract_package_key)
            {
                Ok(Some(StoredValue::ContractPackage(smart_contract_package))) => {
                    match smart_contract_package.versions().last_key_value() {
                        Some((_, hash)) => {
                            let key = Key::Hash(hash.value());
                            (contract_package_addr, key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_package_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(CALLEE_NOT_CALLABLE);
                        }
                    }
                }
                Ok(Some(other)) => panic!("should be smart contract but got {other:?}"),
                Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
                Err(error) => {
                    error!(
                        ?error,
                        ?smart_contract_package_key,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        addressable_entity_key @ Key::SmartContract(smart_contract_addr) => {
            let smart_contract_key = addressable_entity_key;
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressable_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressable_entity_hash.value(),
                            ));
                            (smart_contract_addr, key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(CALLEE_NOT_CALLABLE);
                        }
                    }
                }
                Ok(Some(other)) => panic!("should be smart contract but got {other:?}"),
                Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
                Err(error) => {
                    error!(
                        ?error,
                        ?smart_contract_key,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        other => panic!("should be account or addressable entity but got {other:?}"),
    };
    match caller
        .context_mut()
        .tracking_copy
        .read(&callee_addressable_entity_key)
    {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
            let package_hash = addressable_entity.package_hash();

            let package_key = Key::SmartContract(package_hash.value());
            let mut package = match caller.context_mut().tracking_copy.read(&package_key) {
                Ok(Some(StoredValue::SmartContract(package))) => package,
                Ok(Some(other)) => panic!("should be package but got {other:?}"),
                Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
                Err(error) => {
                    error!(
                        ?error,
                        ?package_hash,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            };

            if package.is_locked() {
                return Ok(CALLEE_NOT_CALLABLE);
            }

            match package.current_entity_hash() {
                Some(previous_hash) => {
                    let protocol_version = caller
                        .context()
                        .runtime_native_config
                        .protocol_version()
                        .value();
                    let next_version = package.next_entity_version_for(protocol_version.major);
                    let new_version_hash_addr =
                        compute_next_contract_hash_version(previous_hash.value(), next_version);
                    package.insert_entity_version(
                        protocol_version.major,
                        EntityAddr::SmartContract(new_version_hash_addr),
                    );
                    if package.disable_entity_version(previous_hash).is_err() {
                        return Ok(CALLEE_NOT_CALLABLE);
                    };

                    metered_write(
                        &mut caller,
                        package_key,
                        StoredValue::SmartContract(package),
                    )?;

                    let bytes = code.clone();
                    let new_byte_code_hash = compute_wasm_bytecode_hash(bytes);
                    let bytecode_key =
                        Key::ByteCode(ByteCodeAddr::V2CasperWasm(new_byte_code_hash));
                    metered_write(
                        &mut caller,
                        bytecode_key,
                        StoredValue::ByteCode(ByteCode::new(
                            ByteCodeKind::V2CasperWasm,
                            code.clone().into(),
                        )),
                    )?;

                    let entity = AddressableEntity::new(
                        package_hash,
                        ByteCodeHash::new(new_byte_code_hash),
                        ProtocolVersion::new(protocol_version),
                        addressable_entity.main_purse(),
                        addressable_entity.associated_keys().clone(),
                        addressable_entity.action_thresholds().clone(),
                        EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
                    );
                    let entity_key =
                        Key::AddressableEntity(EntityAddr::SmartContract(new_version_hash_addr));

                    metered_write(
                        &mut caller,
                        entity_key,
                        StoredValue::AddressableEntity(entity),
                    )?;
                }
                None => return Ok(CALLEE_NOT_CALLABLE),
            }
        }
        Ok(Some(StoredValue::Contract(contract))) => {
            let package_hash = contract.contract_package_hash();

            let package_key = Key::Hash(package_hash.value());
            let mut package = match caller.context_mut().tracking_copy.read(&package_key) {
                Ok(Some(StoredValue::ContractPackage(package))) => package,
                Ok(Some(other)) => panic!("should be package but got {other:?}"),
                Ok(None) => return Ok(HOST_ERROR_INVALID_DATA),
                Err(error) => {
                    error!(
                        ?error,
                        ?package_hash,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            };

            if package.is_locked() {
                return Ok(HOST_LOCKED_PACKAGE);
            }

            match package.current_contract_hash() {
                Some(previous_hash) => {
                    let protocol_version = caller
                        .context()
                        .runtime_native_config
                        .protocol_version()
                        .value();
                    let next_version = package.next_contract_version_for(protocol_version.major);
                    let new_version_hash_addr =
                        compute_next_contract_hash_version(previous_hash.value(), next_version);
                    package.insert_contract_version(
                        protocol_version.major,
                        ContractHash::new(new_version_hash_addr),
                    );
                    if package.disable_contract_version(previous_hash).is_err() {
                        return Ok(HOST_ERROR_INVALID_DATA);
                    };

                    metered_write(
                        &mut caller,
                        package_key,
                        StoredValue::ContractPackage(package),
                    )?;

                    let bytes = code.clone();
                    let new_byte_code_hash = compute_wasm_bytecode_hash(bytes);
                    let bytecode_key =
                        Key::ByteCode(ByteCodeAddr::V2CasperWasm(new_byte_code_hash));
                    metered_write(
                        &mut caller,
                        bytecode_key,
                        StoredValue::ByteCode(ByteCode::new(
                            ByteCodeKind::V2CasperWasm,
                            code.clone().into(),
                        )),
                    )?;

                    let entity = Contract::new(
                        package_hash,
                        ContractWasmHash::new(new_byte_code_hash),
                        contract.named_keys().clone(),
                        contract.entry_points().clone(),
                        ProtocolVersion::new(protocol_version),
                    );
                    let smart_contract = Key::Hash(new_version_hash_addr);

                    metered_write(&mut caller, smart_contract, StoredValue::Contract(entity))?;
                }
                None => return Ok(HOST_NO_ACTIVE_CONTRACT),
            }
        }
        Ok(Some(other_entity)) => {
            panic!("Unexpected entity type: {other_entity:?}")
        }
        Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
        Err(error) => {
            panic!("Error while reading from storage; aborting key={callee_addressable_entity_key:?} error={error:?}")
        }
    };

    // 1. Ensure that the new code is valid (maybe?)
    // TODO: Is validating new code worth it if the user pays for the storage anyway? Should we
    // protect users against invalid code?

    // 2. Update the code therefore making hash(new_code) != addressable_entity.bytecode_addr (aka
    //    hash(old_code))

    // 3. Execute upgrade routine (if specified)
    // this code should handle reading old state, and saving new state

    if let Some(entry_point_name) = entry_point {
        // Limit the new VM to remaining gas.
        let gas_limit = caller
            .get_remaining_points()?
            .try_into_remaining()
            .map_err(|_| InternalHostError::TypeConversion)?;

        let execute_request = ExecuteRequestBuilder::default()
            .with_initiator(caller.context().initiator)
            .with_caller_key(caller.context().callee)
            .with_gas_limit(gas_limit)
            .with_execution_kind(ExecutionKind::Stored {
                address: smart_contract_addr,
                entry_point: entry_point_name.clone(),
            })
            .with_input(input_data.unwrap_or_default())
            // Upgrade entry point is executed with zero value as it does not seem to make sense to
            // be able to transfer anything.
            .with_transferred_value(0)
            .with_transaction_hash(caller.context().transaction_hash)
            // We're using shared address generator there as we need to preserve and advance the
            // state of deterministic address generator across chain of calls.
            .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
            .with_chain_name(caller.context().chain_name.clone())
            .with_block_time(caller.context().block_time)
            .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
            .with_block_height(1) // TODO: Carry on block height
            .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
            .with_runtime_native_config(caller.context().runtime_native_config.clone())
            .build()
            .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

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
                effects,
                cache,
                messages,
            }) => {
                // output
                caller.consume_gas(gas_usage.gas_spent())?;

                if let Some(host_error) = host_error {
                    return Ok(host_error.into_u32());
                }

                caller
                    .context_mut()
                    .tracking_copy
                    .apply_changes(effects, cache, messages);

                if let Some(output) = output {
                    info!(
                        ?entry_point_name,
                        ?output,
                        "unexpected output from migration entry point"
                    );
                }
            }
            Err(execute_error) => {
                // Unable to call contract because of execution error or internal host error.
                // This usually means an internal error that should not happen and has to be handled
                // by the contract runtime.
                error!(
                    ?execute_error,
                    ?entry_point_name,
                    smart_contract_addr = base16::encode_lower(&smart_contract_addr),
                    "Failed to execute upgrade entry point"
                );
                return Err(VMError::Execute(execute_error));
            }
        }
    }

    Ok(CALLEE_SUCCEEDED)
}

pub fn casper_env_info<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    info_ptr: u32,
    info_size: u32,
) -> VMResult<u32> {
    let block_time_cost = caller.context().config.host_function_costs().env_info;
    charge_host_function_call(
        &mut caller,
        &block_time_cost,
        [u64::from(info_ptr), u64::from(info_size)],
    )?;

    let (caller_kind, caller_addr) = match &caller.context().caller {
        Key::Account(account_hash) => (EntityKindTag::Account as u32, account_hash.value()),
        Key::SmartContract(smart_contract_addr) => {
            (EntityKindTag::Contract as u32, *smart_contract_addr)
        }
        Key::Hash(hash_addr) => (EntityKindTag::Contract as u32, *hash_addr),
        other => panic!("Unexpected caller: {other:?}"),
    };

    let (callee_kind, callee_addr) = match &caller.context().callee {
        Key::Account(initiator_addr) => (EntityKindTag::Account as u32, initiator_addr.value()),
        Key::SmartContract(smart_contract_addr) => {
            (EntityKindTag::Contract as u32, *smart_contract_addr)
        }
        Key::Hash(hash_addr) => (EntityKindTag::Contract as u32, *hash_addr),
        other => panic!("Unexpected callee: {other:?}"),
    };

    let transferred_value = caller.context().transferred_value;

    let block_time = caller.context().block_time.value();
    let protocol_version = caller
        .context()
        .runtime_native_config
        .protocol_version()
        .value();
    let parent_block_hash = caller.context().parent_block_hash;
    let block_height = caller.context().block_height;
    // `EnvInfo` in little-endian representation.
    let env_info = EnvInfo {
        caller_addr,
        caller_kind,
        callee_addr,
        callee_kind: callee_kind.to_le(),
        transferred_value: transferred_value.to_le(),
        block_time: block_time.to_le(),
        protocol_version_major: protocol_version.major.to_le(),
        protocol_version_minor: protocol_version.minor.to_le(),
        protocol_version_patch: protocol_version.patch.to_le(),
        parent_block_hash,
        block_height,
    };

    let env_info_bytes = borsh::to_vec(&env_info).map_err(|_| InternalHostError::Serialization)?;
    let write_len = env_info_bytes.len().min(info_size as usize);
    caller.memory_write(info_ptr.wrapped_try_into()?, &env_info_bytes[..write_len])?;

    Ok(HOST_ERROR_SUCCESS)
}

pub fn casper_emit<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    topic_name_ptr: u32,
    topic_name_size: u32,
    payload_ptr: u32,
    payload_size: u32,
) -> VMResult<u32> {
    // In restricted mode, emitting messages is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    // Charge for parameter weights.
    let emit_host_function = caller.context().config.host_function_costs().emit;

    charge_host_function_call(
        &mut caller,
        &emit_host_function,
        [
            u64::from(topic_name_ptr),
            u64::from(topic_name_size),
            u64::from(payload_ptr),
            u64::from(payload_size),
        ],
    )?;

    if topic_name_size > caller.context().message_limits.max_topic_name_size {
        return Ok(HOST_ERROR_TOPIC_TOO_LONG);
    }

    if payload_size > caller.context().message_limits.max_message_size {
        return Ok(HOST_ERROR_PAYLOAD_TOO_LONG);
    }

    let topic_name = {
        let topic: Vec<u8> =
            caller.memory_read(topic_name_ptr.wrapped_try_into()?, topic_name_size as usize)?;
        let Ok(topic) = String::from_utf8(topic) else {
            // Not a valid UTF-8 string
            return Ok(HOST_ERROR_INVALID_DATA);
        };
        topic
    };

    let payload = caller.memory_read(payload_ptr.wrapped_try_into()?, payload_size as usize)?;

    let entity_addr = context_to_entity_addr(caller.context());

    let mut message_topics = caller
        .context_mut()
        .tracking_copy
        .get_message_topics(entity_addr)
        .unwrap_or_else(|error| {
            panic!("Error while reading from storage; aborting error={error:?}")
        });

    if message_topics.len() >= caller.context().message_limits.max_topics_per_contract as usize {
        return Ok(HOST_ERROR_TOO_MANY_TOPICS);
    }

    let topic_name_hash = Digest::hash(&topic_name).value().into();

    match message_topics.add_topic(&topic_name, topic_name_hash) {
        Ok(()) => {
            // New topic is created
        }
        Err(MessageTopicError::DuplicateTopic) => {
            // We're lazily creating message topics and this operation is idempotent.
            // Therefore, already existing topic is not an issue.
        }
        Err(MessageTopicError::MaxTopicsExceeded) => {
            // We're validating the size of topics before adding them
            return Ok(HOST_ERROR_TOO_MANY_TOPICS);
        }
        Err(MessageTopicError::TopicNameSizeExceeded) => {
            // We're validating the length of topic before adding it
            return Ok(HOST_ERROR_TOPIC_TOO_LONG);
        }
        Err(error) => {
            // These error variants are non_exhaustive, and we should handle them explicitly.
            unreachable!("Unexpected error while adding a topic: {:?}", error);
        }
    };

    let current_block_time = caller.context().block_time;
    eprintln!("📩 {topic_name}: {payload:?} (at {current_block_time:?})");

    let topic_key = Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash));
    let prev_topic_summary = match caller.context_mut().tracking_copy.read(&topic_key) {
        Ok(Some(StoredValue::MessageTopic(message_topic_summary))) => message_topic_summary,
        Ok(Some(stored_value)) => {
            panic!("Unexpected stored value: {stored_value:?}");
        }
        Ok(None) => {
            let message_topic_summary =
                MessageTopicSummary::new(0, current_block_time, topic_name.clone());
            let summary = StoredValue::MessageTopic(message_topic_summary.clone());
            caller.context_mut().tracking_copy.write(topic_key, summary);
            message_topic_summary
        }
        Err(error) => panic!("Error while reading from storage; aborting error={error:?}"),
    };

    let topic_message_index = if prev_topic_summary.blocktime() != current_block_time {
        for index in 1..prev_topic_summary.message_count() {
            let message_key = Key::message(entity_addr, topic_name_hash, index);
            debug_assert!(
                {
                    // NOTE: This assertion is to ensure that the message index is continuous, and
                    // the previous messages are pruned properly.
                    caller
                        .context_mut()
                        .tracking_copy
                        .read(&message_key)
                        .map_err(|_| VMError::Internal(InternalHostError::TrackingCopy))?
                        .is_some()
                },
                "Message index is not continuous"
            );

            // Prune the previous messages
            caller.context_mut().tracking_copy.prune(message_key);
        }
        0
    } else {
        prev_topic_summary.message_count()
    };

    // Data stored in the global state associated with the message block.
    type MessageCountPair = (BlockTime, u64);

    let block_message_index: u64 = match caller
        .context_mut()
        .tracking_copy
        .read(&Key::BlockGlobal(BlockGlobalAddr::MessageCount))
    {
        Ok(Some(StoredValue::CLValue(value_pair))) => {
            let (prev_block_time, prev_count): MessageCountPair =
                CLValue::into_t(value_pair).map_err(|_| InternalHostError::TypeConversion)?;
            if prev_block_time == current_block_time {
                prev_count
            } else {
                0
            }
        }
        Ok(Some(other)) => panic!("Unexpected stored value: {other:?}"),
        Ok(None) => {
            // No messages in current block yet
            0
        }
        Err(error) => {
            panic!("Error while reading from storage; aborting error={error:?}")
        }
    };

    let Some(topic_message_count) = topic_message_index.checked_add(1) else {
        return Ok(HOST_ERROR_MESSAGE_TOPIC_FULL);
    };

    let Some(block_message_count) = block_message_index.checked_add(1) else {
        return Ok(HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED);
    };

    // Under v2 runtime messages are only limited to bytes.
    let message_payload = MessagePayload::Bytes(payload.into());

    let message = Message::new(
        entity_addr,
        message_payload,
        topic_name,
        topic_name_hash,
        topic_message_index,
        block_message_index,
    );
    let topic_value = StoredValue::MessageTopic(MessageTopicSummary::new(
        topic_message_count,
        current_block_time,
        message.topic_name().to_owned(),
    ));

    let message_key = message.message_key();
    let message_value = StoredValue::Message(
        message
            .checksum()
            .map_err(|_| InternalHostError::MessageChecksumMissing)?,
    );
    let message_count_pair: MessageCountPair = (current_block_time, block_message_count);
    let block_message_count_value = StoredValue::CLValue(
        CLValue::from_t(message_count_pair).map_err(|_| InternalHostError::TypeConversion)?,
    );

    // Charge for amount as measured by serialized length
    let bytes_count = topic_value.serialized_length()
        + message_value.serialized_length()
        + block_message_count_value.serialized_length();
    charge_gas_storage(&mut caller, bytes_count)?;

    caller.context_mut().tracking_copy.emit_message(
        topic_key,
        topic_value,
        message_key,
        message_value,
        block_message_count_value,
        message,
    );

    Ok(HOST_ERROR_SUCCESS)
}

/// Computes digest hash, using provided algorithm type.
///
/// # Arguments
///
/// * `in_ptr` - pointer to the location where argument bytes will be copied from the host side
/// * `in_size` - size of output pointer
/// * `hash_algo_type` - integer representation of HashAlgorithm enum variant
/// * `out_ptr` - pointer to the location where argument bytes will be copied to the host side
pub fn casper_generic_hash<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    in_ptr: u32,
    in_size: u32,
    hash_algorithm: u32,
    out_ptr: u32,
) -> VMResult<u32> {
    const DIGEST_LENGTH: usize = 32;

    let in_bytes: Vec<u8> = caller.memory_read(in_ptr.wrapped_try_into()?, in_size as usize)?;

    // Charge for parameter weights.
    let generic_hash_cost = caller.context().config.host_function_costs().generic_hash;

    charge_host_function_call(
        &mut caller,
        &generic_hash_cost,
        [
            u64::from(in_ptr),
            u64::from(in_size),
            u64::from(out_ptr),
            u64::from(hash_algorithm),
        ],
    )?;

    let hash_algorithm =
        HashAlgorithm::from_u32(hash_algorithm).ok_or(InternalHostError::TypeConversion)?;

    let hashed_bytes = match hash_algorithm {
        HashAlgorithm::Blake2b => {
            let mut result = [0; DIGEST_LENGTH];
            let mut hasher = Blake2bVar::new(DIGEST_LENGTH).map_err(|_| {
                ExecuteError::InternalHost(InternalHostError::CorruptExecutionState(
                    "Error when creating instance of Blake2bVar hashing".to_owned(),
                ))
            })?;
            hasher.update(in_bytes.as_ref());
            hasher.finalize_variable(&mut result).ok();
            result
        }
        HashAlgorithm::Blake3 => {
            let mut result = [0; DIGEST_LENGTH];
            let mut hasher = blake3::Hasher::new();
            hasher.update(in_bytes.as_ref());
            let hash = hasher.finalize();
            let hash_bytes: &[u8; DIGEST_LENGTH] = hash.as_bytes();
            result.copy_from_slice(hash_bytes);
            result
        }
        HashAlgorithm::Sha256 => Sha256::digest(in_bytes).into(),
        HashAlgorithm::Keccak256 => {
            use keccak_asm::Keccak256;
            let mut result = [0u8; DIGEST_LENGTH];
            let mut hasher = Keccak256::new();
            KeccakDigest::update(&mut hasher, &in_bytes);
            let hash = KeccakDigest::finalize(hasher);
            result.copy_from_slice(&hash);
            result
        }
    };

    caller.memory_write(out_ptr.wrapped_try_into()?, &hashed_bytes)?;

    Ok(HOST_ERROR_SUCCESS)
}

/// Recovers a Secp256k1 public key from a signed message
/// and a signature used in the process of signing.
///
/// # Arguments
///
/// * `message_ptr` - pointer to the signed data
/// * `message_size` - length of the signed data in bytes
/// * `signature_ptr` - pointer to byte-encoded signature
/// * `signature_size` - length of the byte-encoded signature
/// * `public_key_ptr` - pointer to a buffer of size PublicKey::SECP256K1_LENGTH which will be
///   populated with the recovered key's bytes representation
/// * `recovery_id` - an integer value 0, 1, 2, or 3 used to select the correct public key from the
///   signature:
///   - Low bit (0/1): was the y-coordinate of the affine point resulting from the fixed-base
///     multiplication 𝑘×𝑮 odd?
///   - Hi bit (3/4): did the affine x-coordinate of 𝑘×𝑮 overflow the order of the scalar field,
///     requiring a reduction when computing r?
pub fn casper_recover_secp256k1<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    message_ptr: u32,
    message_size: u32,
    signature_ptr: u32,
    signature_size: u32,
    public_key_ptr: u32,
    recovery_id: u32,
) -> VMResult<u32> {
    let recover_secp256k1_cost = caller
        .context()
        .config
        .host_function_costs()
        .recover_secp256k1;

    charge_host_function_call(
        &mut caller,
        &recover_secp256k1_cost,
        [
            u64::from(message_ptr),
            u64::from(message_size),
            u64::from(signature_ptr),
            u64::from(signature_size),
            u64::from(public_key_ptr),
            u64::from(recovery_id),
        ],
    )?;

    if recovery_id >= 4 {
        return Ok(HOST_ERROR_INVALID_INPUT);
    }

    let message = caller.memory_read(message_ptr.wrapped_try_into()?, message_size as usize)?;
    let signature_bytes =
        caller.memory_read(signature_ptr.wrapped_try_into()?, signature_size as usize)?;
    let Ok((signature, _)) = Signature::from_bytes(&signature_bytes) else {
        return Ok(HOST_ERROR_INVALID_DATA);
    };

    let Ok(public_key) =
        casper_types::crypto::recover_secp256k1(message, &signature, recovery_id as u8)
    else {
        return Ok(HOST_ERROR_INVALID_INPUT);
    };

    let Ok(key_bytes) = public_key.to_bytes() else {
        return Ok(HOST_ERROR_PAYLOAD_TOO_LONG);
    };

    caller.memory_write(public_key_ptr.wrapped_try_into()?, &key_bytes)?;

    Ok(HOST_ERROR_SUCCESS)
}
