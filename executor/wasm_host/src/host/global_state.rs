use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use bytes::Bytes;
use casper_executor_wasm_common::{
    chain_utils::{self, compute_next_contract_hash_version},
    entry_point::{
        ENTRY_POINT_PAYMENT_CALLER, ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY,
        ENTRY_POINT_PAYMENT_SELF_ONWARD,
    },
    error::{
        CALLEE_SUCCEEDED, CALLEE_TRAPPED, HOST_ERROR_CL_VALUE, HOST_ERROR_INVALID_DATA,
        HOST_ERROR_INVALID_INPUT, HOST_ERROR_NOT_FOUND, HOST_ERROR_SUCCESS,
    },
    keyspace::{Keyspace, KeyspaceTag},
};
use casper_executor_wasm_interface::{
    executor::{ExecuteRequestBuilder, ExecuteResult, ExecutionKind, Executor},
    Caller, FatalHostError, VMError, VMResult,
};
use casper_storage::{global_state::GlobalStateReader, tracking_copy::TrackingCopyExt};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, NamedKeyAddr, NamedKeyValue},
    bytesrepr::{self, Bytes as BytesreprBytes, ToBytes},
    contracts::{ContractHash, ContractPackage, ContractPackageHash, EntryPoints},
    AccessRights, AddressableEntity, BlockHash, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind,
    CLType, CLValue, Contract, ContractRuntimeTag, ContractWasmHash, Digest, EntityAddr,
    EntityKind, EntryPointPayment, EntryPointValue, HashAddr, Key, NamedKeys, Package, PackageAddr,
    ProtocolVersion, StoredValue, URef,
};
use either::Either;
use num_traits::FromPrimitive;
use tracing::{debug, error, warn};

use crate::{
    abi::{CreateResult, EnvInfo},
    context::Context,
    host::{context_to_entity_addr, metered_write, EntityKindTag, NAME_FOR_V2_CONTRACT_MAIN_PURSE},
    system,
};

/// Read value under from global state under a key.
pub(crate) fn host_read<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<(Option<Bytes>, u32)> {
    let (key_tag, key_payload_bytes) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u64, BytesreprBytes)>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok((None, HOST_ERROR_INVALID_INPUT));
            }
        };
    let keyspace_tag = match KeyspaceTag::from_u64(key_tag) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok((None, HOST_ERROR_INVALID_INPUT));
        }
    };

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok((None, HOST_ERROR_INVALID_DATA));
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
            return Ok((None, HOST_ERROR_NOT_FOUND));
        }
    };

    let global_state_read_result = caller.context_mut().tracking_copy.read(&global_state_key);
    let global_state_raw_bytes: Cow<[u8]> = match global_state_read_result {
        Ok(Some(StoredValue::CLValue(cl_value))) => {
            let CLType::Any = cl_value.cl_type() else {
                return Err(FatalHostError::TypeConversion)?;
            };
            Cow::Owned(cl_value.inner_bytes().to_owned())
        }
        Ok(Some(StoredValue::NamedKey(named_key_value))) => {
            // Dereference named key to its URef and return the underlying Any bytes
            let Ok(Key::URef(uref)) = named_key_value.get_key() else {
                return Ok((None, HOST_ERROR_INVALID_DATA));
            };

            match caller.context_mut().tracking_copy.read(&Key::URef(uref)) {
                Ok(Some(StoredValue::CLValue(cl_value))) => {
                    let CLType::Any = cl_value.cl_type() else {
                        return Ok((None, HOST_ERROR_INVALID_DATA));
                    };
                    Cow::Owned(cl_value.inner_bytes().to_owned())
                }
                Ok(Some(_)) => {
                    return Ok((None, HOST_ERROR_INVALID_DATA));
                }
                Ok(None) => {
                    return Ok((None, HOST_ERROR_NOT_FOUND));
                }
                Err(_error) => {
                    return Err(FatalHostError::TrackingCopy.into());
                }
            }
        }
        Ok(Some(StoredValue::Contract(contract))) => match keyspace {
            Keyspace::NamedKey(name) => {
                let Some(Key::URef(uref)) = contract.named_keys().get(name) else {
                    return Ok((None, HOST_ERROR_INVALID_DATA));
                };

                match caller.context_mut().tracking_copy.read(&Key::URef(*uref)) {
                    Ok(Some(StoredValue::CLValue(cl_value))) => {
                        let CLType::Any = cl_value.cl_type() else {
                            return Ok((None, HOST_ERROR_INVALID_DATA));
                        };
                        Cow::Owned(cl_value.inner_bytes().to_owned())
                    }
                    Ok(Some(_)) => {
                        return Ok((None, HOST_ERROR_INVALID_DATA));
                    }
                    Ok(None) => {
                        return Ok((None, HOST_ERROR_NOT_FOUND));
                    }
                    Err(_error) => {
                        return Err(FatalHostError::TrackingCopy.into());
                    }
                }
            }
            Keyspace::AllNamedKeys => match contract.take_named_keys().to_bytes() {
                Ok(bytes) => Cow::Owned(bytes),
                Err(_) => return Ok((None, HOST_ERROR_INVALID_INPUT)),
            },
            _ => {
                error!(?keyspace, "unsupported keyspace");
                return Ok((None, HOST_ERROR_INVALID_INPUT));
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
                    Ok(_) | Err(_) => return Ok((None, HOST_ERROR_INVALID_INPUT)),
                }
            } else {
                return Ok((None, HOST_ERROR_INVALID_INPUT));
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
        Ok(None) => return Ok((None, HOST_ERROR_NOT_FOUND)), // Entry does not exist
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
    Ok((
        Some(Bytes::from(global_state_raw_bytes.to_vec())),
        HOST_ERROR_SUCCESS,
    ))
}

/// Write value under a key.
pub(crate) fn host_write<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<u32> {
    let (key_space, key_payload_bytes, value) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u64, BytesreprBytes, BytesreprBytes)>(
            &input,
        ) {
            Ok(res) => res,
            Err(_) => {
                return Ok(HOST_ERROR_INVALID_INPUT);
            }
        };
    let value = value.take_inner();
    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_INVALID_INPUT);
        }
    };

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(HOST_ERROR_INVALID_INPUT);
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
                .map_err(|_| FatalHostError::TrackingCopy)?;

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
                    metered_write(caller, Key::URef(uref), StoredValue::CLValue(cl_value_any))?;

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
                    metered_write(caller, Key::URef(uref), StoredValue::CLValue(cl_value_any))?;

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

    metered_write(caller, global_state_key, stored_value)?;

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
pub(crate) fn host_remove<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<u32> {
    let (key_space, key_payload_bytes) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u64, BytesreprBytes)>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok(HOST_ERROR_INVALID_INPUT);
            }
        };

    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

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
            debug!(
                ?error,
                ?global_state_key,
                "Error while attempting a read before removing value; aborting"
            );
            return Err(VMError::Fatal(FatalHostError::TrackingCopy));
        }
    }

    Ok(HOST_ERROR_SUCCESS)
}

pub(crate) fn host_env_balance<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<(Option<Bytes>, u32)> {
    let (entity_kind, entity_addr) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u32, [u8; 32])>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok((None, HOST_ERROR_INVALID_INPUT));
            }
        };

    let entity_key = match EntityKindTag::from_u32(entity_kind) {
        Some(EntityKindTag::Account) => {
            let account_hash: AccountHash = AccountHash::new(entity_addr);

            let account_key = Key::Account(account_hash);
            match caller.context_mut().tracking_copy.read(&account_key) {
                Ok(Some(StoredValue::CLValue(clvalue))) => {
                    let addressable_entity_key = clvalue
                        .into_t::<Key>()
                        .map_err(|_| FatalHostError::TypeConversion)?;
                    Either::Right(addressable_entity_key)
                }
                Ok(Some(StoredValue::Account(account))) => Either::Left(account.main_purse()),
                Ok(Some(other_entity)) => {
                    error!("Unexpected entity type: {other_entity:?}");
                    return Err(FatalHostError::UnexpectedEntityKind.into());
                }
                Ok(None) => return Ok((None, HOST_ERROR_SUCCESS)),
                Err(error) => {
                    error!("Error while reading from storage; aborting key={account_key:?} error={error:?}");
                    return Err(FatalHostError::TrackingCopy.into());
                }
            }
        }
        Some(EntityKindTag::Contract) => {
            let hash_bytes = entity_addr;
            let smart_contract_key = if caller.context().tracking_copy.addressable_entity_enabled()
            {
                Key::Package(hash_bytes.into())
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
                            return Ok((None, HOST_ERROR_SUCCESS));
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
                            return Ok((None, HOST_ERROR_NOT_FOUND));
                        }
                    }
                }
                Ok(Some(_)) => {
                    return Ok((None, HOST_ERROR_SUCCESS));
                }
                Ok(None) => {
                    // Not found, balance is 0
                    return Ok((None, HOST_ERROR_SUCCESS));
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
        None => return Ok((None, HOST_ERROR_SUCCESS)),
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
                            return Ok((None, HOST_ERROR_SUCCESS));
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
        .map_err(|_| FatalHostError::TotalBalanceReadFailure)?;

    let total_balance: u64 = total_balance
        .value()
        .try_into()
        .map_err(|_| FatalHostError::TotalBalanceOverflow)?;

    Ok((
        Some(Bytes::from(total_balance.to_le_bytes().to_vec())),
        HOST_ERROR_SUCCESS,
    ))
}

pub(crate) fn host_env_info<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
) -> VMResult<(Option<Bytes>, u32)> {
    let (caller_kind, caller_addr) = match &caller.context().caller {
        Key::Account(account_hash) => (EntityKindTag::Account as u32, account_hash.value()),
        Key::Package(smart_contract_addr) => {
            (EntityKindTag::Contract as u32, smart_contract_addr.value())
        }
        Key::Hash(hash_addr) => (EntityKindTag::Contract as u32, *hash_addr),
        other => panic!("Unexpected caller: {other:?}"),
    };

    let (callee_kind, callee_addr) = match &caller.context().callee {
        Key::Account(initiator_addr) => (EntityKindTag::Account as u32, initiator_addr.value()),
        Key::Package(smart_contract_addr) => {
            (EntityKindTag::Contract as u32, smart_contract_addr.value())
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
    let env_info = EnvInfo {
        protocol_version_major: protocol_version.major.to_le(),
        protocol_version_minor: protocol_version.minor.to_le(),
        protocol_version_patch: protocol_version.patch.to_le(),
        block_height,
        block_time: block_time.to_le(),
        parent_block_hash,
        transferred_value: transferred_value.to_le(),
        caller_addr,
        caller_kind,
        callee_addr,
        callee_kind: callee_kind.to_le(),
    };

    let env_info_bytes = borsh::to_vec(&env_info).map_err(|_| FatalHostError::Serialization)?;
    Ok((Some(Bytes::from(env_info_bytes)), HOST_ERROR_SUCCESS))
}

pub(crate) fn host_create<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<(Option<Bytes>, u32)> {
    let (
        transferred_value,
        maybe_inputed_bytecode,
        maybe_seed,
        maybe_constructor_name,
        constructor_data,
    ) = match bytesrepr::deserialize_from_slice::<
        &Bytes,
        (
            u64,
            Option<BytesreprBytes>,
            Option<[u8; 32]>,
            Option<String>,
            Option<BytesreprBytes>,
        ),
    >(&input)
    {
        Ok(res) => res,
        Err(_) => {
            return Ok((None, HOST_ERROR_INVALID_INPUT));
        }
    };
    let constructor_data = constructor_data.map(|x| Bytes::from(x.take_inner()));

    let bytecode = if let Some(bytecode) = maybe_inputed_bytecode {
        Bytes::from(bytecode.take_inner())
    } else {
        caller.bytecode()
    };

    let bytecode_hash = chain_utils::compute_wasm_bytecode_hash(&bytecode);

    let bytecode = ByteCode::new(ByteCodeKind::V2CasperWasm, bytecode.clone().into());
    let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash);

    let callee_addr = context_to_entity_addr(caller.context()).value();

    let package_addr: HashAddr = chain_utils::compute_predictable_address(
        caller.context().chain_name.as_bytes(),
        callee_addr,
        bytecode_hash,
        maybe_seed,
    );

    let protocol_version = ProtocolVersion::V2_0_0;
    let protocol_version_major = protocol_version.value().major;

    let ae_enabled = caller.context().tracking_copy.addressable_entity_enabled();

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
                Key::Package(package_addr.into()),
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
        .map_err(|_| VMError::Fatal(FatalHostError::TrackingCopy))?
        .is_some()
    {
        return Err(VMError::Fatal(FatalHostError::ContractAlreadyExists));
    }

    metered_write(
        caller,
        smart_contract_package_key,
        smart_contract_package_as_stored_value,
    )?;

    // 2. Store wasm
    if !ae_enabled {
        let byte_code_key = Key::byte_code_key(ByteCodeAddr::V2CasperWasm(bytecode_hash));
        let byte_code_key_as_cl_value = match CLValue::from_t(byte_code_key) {
            Ok(cl_value) => cl_value,
            Err(_) => return Ok((None, HOST_ERROR_CL_VALUE)),
        };

        metered_write(
            caller,
            Key::Hash(bytecode_hash),
            StoredValue::CLValue(byte_code_key_as_cl_value),
        )?
    };

    metered_write(
        caller,
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
            return Ok((None, CALLEE_TRAPPED));
        }
    };

    if ae_enabled {
        // 3. Store addressable entity
        let entity_addr = EntityAddr::SmartContract(smart_contract_addr);
        let addressable_entity_key = Key::AddressableEntity(entity_addr);

        let addressable_entity = AddressableEntity::new(
            PackageAddr::new(package_addr),
            ByteCodeHash::new(bytecode_hash),
            ProtocolVersion::V2_0_0,
            main_purse,
            AssociatedKeys::default(),
            ActionThresholds::default(),
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
        );

        metered_write(
            caller,
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
            caller,
            Key::Hash(smart_contract_addr),
            StoredValue::Contract(contract),
        )?;
    }

    let _initial_state = match maybe_constructor_name {
        Some(entry_point_name) => {
            // Limit the new VM to remaining gas.
            let gas_limit = caller
                .get_remaining_points()?
                .try_into_remaining()
                .map_err(|_| FatalHostError::TypeConversion)?;

            let execute_request = ExecuteRequestBuilder::default()
                .with_initiator(caller.context().initiator)
                .with_caller_key(caller.context().callee)
                .with_gas_limit(gas_limit)
                .with_execution_kind(ExecutionKind::Stored {
                    address: package_addr,
                    entry_point: entry_point_name.clone(),
                })
                .with_input(constructor_data.unwrap_or_default())
                .with_transferred_value(transferred_value)
                .with_transaction_hash(caller.context().transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the
                // state of deterministic address generator across chain of calls.
                .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
                .with_chain_name(caller.context().chain_name.clone())
                .with_block_time(caller.context().block_time)
                .with_state_hash(Digest::from_raw([0; 32]))
                .with_block_height(1)
                .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
                .with_runtime_native_config(caller.context().runtime_native_config.clone())
                .with_authorization_keys(caller.context().authorization_keys.clone())
                .build()
                .map_err(FatalHostError::ExecuteRequestBuildFailure)?;

            let tracking_copy_for_ctor = caller.context().tracking_copy.fork2();

            match caller
                .executor()
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
                        return Ok((None, host_error.into_u32()));
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

    let create_result = CreateResult { package_addr };

    let create_result_bytes =
        borsh::to_vec(&create_result).map_err(|_| FatalHostError::Serialization)?;

    Ok((Some(create_result_bytes.into()), CALLEE_SUCCEEDED))
}

fn keyspace_to_global_state_key<S: GlobalStateReader>(
    context: &Context<S>,
    keyspace: Keyspace<'_>,
) -> Option<Key> {
    let entity_addr = context_to_entity_addr(context);
    let ae_enabled = context.tracking_copy.addressable_entity_enabled();

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
