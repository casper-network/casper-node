use std::sync::Arc;

use bytes::Bytes;
use casper_executor_wasm_common::{
    chain_utils::{compute_next_contract_hash_version, compute_wasm_bytecode_hash},
    error::{
        CALLEE_INPUT_INVALID, CALLEE_LOCKED_PACKAGE, CALLEE_NOT_CALLABLE,
        CALLEE_NO_ACTIVE_CONTRACT, CALLEE_SUCCEEDED, HOST_ERROR_INVALID_DATA,
    },
};
use casper_executor_wasm_interface::{
    executor::{ExecuteError, ExecuteRequestBuilder, ExecuteResult, ExecutionKind, Executor},
    Caller, FatalHostError, VMError, VMResult,
};
use casper_storage::global_state::GlobalStateReader;
use casper_types::{
    bytesrepr::{self, Bytes as BytesreprBytes},
    contracts::ContractHash,
    AddressableEntity, BlockHash, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, Contract,
    ContractRuntimeTag, ContractWasmHash, Digest, EntityAddr, EntityKind, HashAddr, Key,
    ProtocolVersion, StoredValue,
};
use tracing::{error, info, warn};

use crate::{
    context::Context,
    host::{exec, metered_write},
};

pub(crate) fn host_call<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<(Option<Bytes>, u32)> {
    let (smart_contract_addr, input_data, entry_point_name, transferred_value) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (HashAddr, BytesreprBytes, String, u64)>(
            &input,
        ) {
            Ok(res) => res,
            Err(_) => {
                return Ok((None, CALLEE_INPUT_INVALID));
            }
        };
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

    let input_data = Bytes::from(input_data.take_inner());

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
            address: smart_contract_addr,
            entry_point: entry_point_name.clone(),
        })
        .with_transferred_value(transferred_value)
        .with_input(input_data)
        .with_transaction_hash(caller.context().transaction_hash)
        // We're using shared address generator there as we need to preserve and advance the state
        // of deterministic address generator across chain of calls.
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

    let ret = exec(caller, execute_request);
    if let Err(execute_error) = &ret {
        error!(
            ?execute_error,
            ?smart_contract_addr,
            ?entry_point_name,
            "Failed to execute entry point"
        );
    }
    ret
}

pub(crate) fn host_upgrade<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<u32> {
    let (code, entry_point_name, input_data) = match bytesrepr::deserialize_from_slice::<
        &Bytes,
        (BytesreprBytes, Option<String>, Option<BytesreprBytes>),
    >(&input)
    {
        Ok(res) => res,
        Err(_) => {
            return Ok(CALLEE_INPUT_INVALID);
        }
    };
    let code: Bytes = Bytes::from(code.take_inner());

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> =
        input_data.map(|input_data| Bytes::from(input_data.take_inner()));

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
        addressable_entity_key @ Key::Package(smart_contract_addr) => {
            let smart_contract_key = addressable_entity_key;
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressable_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressable_entity_hash.value(),
                            ));
                            (smart_contract_addr.value(), key)
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
    let (contract_key, package_key, wasm_key, version_major, version_minor) = match caller
        .context_mut()
        .tracking_copy
        .read(&callee_addressable_entity_key)
    {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
            let package_addr = addressable_entity.package();

            let package_key = Key::Package(package_addr);
            let mut package = match caller.context_mut().tracking_copy.read(&package_key) {
                Ok(Some(StoredValue::SmartContract(package))) => package,
                Ok(Some(other)) => panic!("should be package but got {other:?}"),
                Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
                Err(error) => {
                    error!(
                        ?error,
                        ?package_addr,
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
                    let entity_version_key = package.insert_entity_version(
                        protocol_version.major,
                        EntityAddr::SmartContract(new_version_hash_addr),
                    );
                    if package.disable_entity_version(previous_hash).is_err() {
                        return Ok(CALLEE_NOT_CALLABLE);
                    };

                    metered_write(caller, package_key, StoredValue::SmartContract(package))?;

                    let bytes = code.clone();
                    let new_byte_code_hash = compute_wasm_bytecode_hash(bytes);
                    let bytecode_key =
                        Key::ByteCode(ByteCodeAddr::V2CasperWasm(new_byte_code_hash));
                    metered_write(
                        caller,
                        bytecode_key,
                        StoredValue::ByteCode(ByteCode::new(
                            ByteCodeKind::V2CasperWasm,
                            code.clone().into(),
                        )),
                    )?;

                    let entity = AddressableEntity::new(
                        package_addr,
                        ByteCodeHash::new(new_byte_code_hash),
                        ProtocolVersion::new(protocol_version),
                        addressable_entity.main_purse(),
                        addressable_entity.associated_keys().clone(),
                        addressable_entity.action_thresholds().clone(),
                        EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
                    );
                    let entity_key =
                        Key::AddressableEntity(EntityAddr::SmartContract(new_version_hash_addr));

                    metered_write(caller, entity_key, StoredValue::AddressableEntity(entity))?;
                    (
                        entity_key,
                        package_key,
                        bytecode_key,
                        entity_version_key.protocol_version_major(),
                        entity_version_key.entity_version(),
                    )
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
                return Ok(CALLEE_LOCKED_PACKAGE);
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
                    let contract_version_key = package.insert_contract_version(
                        protocol_version.major,
                        ContractHash::new(new_version_hash_addr),
                    );
                    if package.disable_contract_version(previous_hash).is_err() {
                        return Ok(HOST_ERROR_INVALID_DATA);
                    };

                    metered_write(caller, package_key, StoredValue::ContractPackage(package))?;

                    let bytes = code.clone();
                    let new_byte_code_hash = compute_wasm_bytecode_hash(bytes);
                    let bytecode_key =
                        Key::ByteCode(ByteCodeAddr::V2CasperWasm(new_byte_code_hash));
                    metered_write(
                        caller,
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

                    metered_write(caller, smart_contract, StoredValue::Contract(entity))?;
                    (
                        smart_contract,
                        package_key,
                        bytecode_key,
                        contract_version_key.protocol_version_major(),
                        contract_version_key.contract_version(),
                    )
                }
                None => return Ok(CALLEE_NO_ACTIVE_CONTRACT),
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

    if let Some(entry_point_name) = entry_point_name {
        // Limit the new VM to remaining gas.
        let gas_limit = caller
            .get_remaining_points()?
            .try_into_remaining()
            .map_err(|_| FatalHostError::TypeConversion)?;
        let block_time = caller.context().block_time;

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
            .with_block_time(block_time)
            .with_state_hash(Digest::from_raw([0; 32]))
            .with_block_height(1)
            .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
            .with_runtime_native_config(caller.context().runtime_native_config.clone())
            .with_authorization_keys(caller.context().authorization_keys.clone())
            .build()
            .map_err(FatalHostError::ExecuteRequestBuildFailure)?;

        let mut tracking_copy_for_ctor = caller.context().tracking_copy.fork2();
        match tracking_copy_for_ctor.emit_messages_for_new_installed_version(
            package_key,
            contract_key,
            wasm_key,
            version_major,
            version_minor,
            block_time,
        ) {
            Ok(_) => (),
            Err(message_emission_error) => {
                return Err(VMError::Execute(ExecuteError::Api(
                    message_emission_error.to_string(),
                )))
            }
        }
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
