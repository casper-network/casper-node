use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
};

use casper_wasmi::{Externals, RuntimeArgs, RuntimeValue, Trap};

use casper_storage::global_state::{error::Error as GlobalStateError, state::StateReader};
use casper_types::{
    account::AccountHash,
    addressable_entity::{EntryPoints, NamedKeys},
    api_error,
    bytesrepr::{self, ToBytes},
    contract_messages::MessageTopicOperation,
    crypto, AddressableEntityHash, ApiError, EntityVersion, Gas, Group, HostFunction,
    HostFunctionCost, Key, PackageHash, PackageStatus, StoredValue, URef,
    DEFAULT_HOST_FUNCTION_NEW_DICTIONARY, U512, UREF_SERIALIZED_LENGTH,
};

use super::{args::Args, ExecError, Runtime};
use crate::resolvers::v1_function_index::FunctionIndex;

impl<'a, R> Externals for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn invoke_index(
        &mut self,
        index: usize,
        args: RuntimeArgs,
    ) -> Result<Option<RuntimeValue>, Trap> {
        let func = FunctionIndex::try_from(index).expect("unknown function index");

        let host_function_costs = self
            .context
            .engine_config()
            .wasm_config()
            .take_host_function_costs();

        match func {
            FunctionIndex::ReadFuncIndex => {
                // args(0) = pointer to key in Wasm memory
                // args(1) = size of key in Wasm memory
                // args(2) = pointer to output size (output param)
                let (key_ptr, key_size, output_size_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.read_value,
                    [key_ptr, key_size, output_size_ptr],
                )?;
                let ret = self.read(key_ptr, key_size, output_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::LoadNamedKeysFuncIndex => {
                // args(0) = pointer to amount of keys (output)
                // args(1) = pointer to amount of serialized bytes (output)
                let (total_keys_ptr, result_size_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.load_named_keys,
                    [total_keys_ptr, result_size_ptr],
                )?;
                let ret = self.load_named_keys(total_keys_ptr, result_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::WriteFuncIndex => {
                // args(0) = pointer to key in Wasm memory
                // args(1) = size of key
                // args(2) = pointer to value
                // args(3) = size of value
                let (key_ptr, key_size, value_ptr, value_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.write,
                    [key_ptr, key_size, value_ptr, value_size],
                )?;
                self.write(key_ptr, key_size, value_ptr, value_size)?;
                Ok(None)
            }

            FunctionIndex::AddFuncIndex => {
                // args(0) = pointer to key in Wasm memory
                // args(1) = size of key
                // args(2) = pointer to value
                // args(3) = size of value
                let (key_ptr, key_size, value_ptr, value_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.add,
                    [key_ptr, key_size, value_ptr, value_size],
                )?;
                self.add(key_ptr, key_size, value_ptr, value_size)?;
                Ok(None)
            }

            FunctionIndex::NewFuncIndex => {
                // args(0) = pointer to uref destination in Wasm memory
                // args(1) = pointer to initial value
                // args(2) = size of initial value
                let (uref_ptr, value_ptr, value_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.new_uref,
                    [uref_ptr, value_ptr, value_size],
                )?;
                self.new_uref(uref_ptr, value_ptr, value_size)?;
                Ok(None)
            }

            FunctionIndex::RetFuncIndex => {
                // args(0) = pointer to value
                // args(1) = size of value
                let (value_ptr, value_size) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.ret, [value_ptr, value_size])?;
                Err(self.ret(value_ptr, value_size as usize))
            }

            FunctionIndex::GetKeyFuncIndex => {
                // args(0) = pointer to key name in Wasm memory
                // args(1) = size of key name
                // args(2) = pointer to output buffer for serialized key
                // args(3) = size of output buffer
                // args(4) = pointer to bytes written
                let (name_ptr, name_size, output_ptr, output_size, bytes_written) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.get_key,
                    [name_ptr, name_size, output_ptr, output_size, bytes_written],
                )?;
                let ret = self.load_key(
                    name_ptr,
                    name_size,
                    output_ptr,
                    output_size as usize,
                    bytes_written,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::HasKeyFuncIndex => {
                // args(0) = pointer to key name in Wasm memory
                // args(1) = size of key name
                let (name_ptr, name_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.has_key,
                    [name_ptr, name_size],
                )?;
                let result = self.has_key(name_ptr, name_size)?;
                Ok(Some(RuntimeValue::I32(result)))
            }

            FunctionIndex::PutKeyFuncIndex => {
                // args(0) = pointer to key name in Wasm memory
                // args(1) = size of key name
                // args(2) = pointer to key in Wasm memory
                // args(3) = size of key
                let (name_ptr, name_size, key_ptr, key_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.put_key,
                    [name_ptr, name_size, key_ptr, key_size],
                )?;
                self.put_key(name_ptr, name_size, key_ptr, key_size)?;
                Ok(None)
            }

            FunctionIndex::RemoveKeyFuncIndex => {
                // args(0) = pointer to key name in Wasm memory
                // args(1) = size of key name
                let (name_ptr, name_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.remove_key,
                    [name_ptr, name_size],
                )?;
                self.remove_key(name_ptr, name_size)?;
                Ok(None)
            }

            FunctionIndex::GetCallerIndex => {
                // args(0) = pointer where a size of serialized bytes will be stored
                let (output_size,) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.get_caller, [output_size])?;
                let ret = self.get_caller(output_size)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::GetBlocktimeIndex => {
                // args(0) = pointer to Wasm memory where to write.
                let (dest_ptr,) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.get_blocktime, [dest_ptr])?;
                self.get_blocktime(dest_ptr)?;
                Ok(None)
            }

            FunctionIndex::GasFuncIndex => {
                let (gas_arg,): (u32,) = Args::parse(args)?;
                // Gas is special cased internal host function and for accounting purposes it isn't
                // represented in protocol data.
                self.gas(Gas::new(gas_arg))?;
                Ok(None)
            }

            FunctionIndex::IsValidURefFnIndex => {
                // args(0) = pointer to value to validate
                // args(1) = size of value
                let (uref_ptr, uref_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.is_valid_uref,
                    [uref_ptr, uref_size],
                )?;
                Ok(Some(RuntimeValue::I32(i32::from(
                    self.is_valid_uref(uref_ptr, uref_size)?,
                ))))
            }

            FunctionIndex::RevertFuncIndex => {
                // args(0) = status u32
                let (status,) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.revert, [status])?;
                Err(self.revert(status))
            }

            FunctionIndex::AddAssociatedKeyFuncIndex => {
                // args(0) = pointer to array of bytes of an account hash
                // args(1) = size of an account hash
                // args(2) = weight of the key
                let (account_hash_ptr, account_hash_size, weight_value) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.add_associated_key,
                    [
                        account_hash_ptr,
                        account_hash_size,
                        weight_value as HostFunctionCost,
                    ],
                )?;
                let value = self.add_associated_key_old(
                    account_hash_ptr,
                    account_hash_size as usize,
                    weight_value,
                )?;
                Ok(Some(RuntimeValue::I32(value)))
            }

            FunctionIndex::RemoveAssociatedKeyFuncIndex => {
                // args(0) = pointer to array of bytes of an account hash
                // args(1) = size of an account hash
                let (account_hash_ptr, account_hash_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.remove_associated_key,
                    [account_hash_ptr, account_hash_size],
                )?;
                let value =
                    self.remove_associated_key_old(account_hash_ptr, account_hash_size as usize)?;
                Ok(Some(RuntimeValue::I32(value)))
            }

            FunctionIndex::UpdateAssociatedKeyFuncIndex => {
                // args(0) = pointer to array of bytes of an account hash
                // args(1) = size of an account hash
                // args(2) = weight of the key
                let (account_hash_ptr, account_hash_size, weight_value) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.update_associated_key,
                    [
                        account_hash_ptr,
                        account_hash_size,
                        weight_value as HostFunctionCost,
                    ],
                )?;
                let value = self.update_associated_key_old(
                    account_hash_ptr,
                    account_hash_size as usize,
                    weight_value,
                )?;
                Ok(Some(RuntimeValue::I32(value)))
            }

            FunctionIndex::SetActionThresholdFuncIndex => {
                // args(0) = action type
                // args(1) = new threshold
                let (action_type_value, threshold_value) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.set_action_threshold,
                    [action_type_value, threshold_value as HostFunctionCost],
                )?;
                let value = self.set_action_threshold(action_type_value, threshold_value)?;
                Ok(Some(RuntimeValue::I32(value)))
            }

            FunctionIndex::CreatePurseIndex => {
                // args(0) = pointer to array for return value
                // args(1) = length of array for return value
                let (dest_ptr, dest_size) = Args::parse(args)?;

                self.charge_host_function_call(
                    &host_function_costs.create_purse,
                    [dest_ptr, dest_size],
                )?;

                let result = if (dest_size as usize) < UREF_SERIALIZED_LENGTH {
                    Err(ApiError::PurseNotCreated)
                } else {
                    let purse = self.create_purse()?;
                    let purse_bytes = purse.into_bytes().map_err(ExecError::BytesRepr)?;
                    self.try_get_memory()?
                        .set(dest_ptr, &purse_bytes)
                        .map_err(|e| ExecError::Interpreter(e.into()))?;
                    Ok(())
                };

                Ok(Some(RuntimeValue::I32(api_error::i32_from(result))))
            }

            FunctionIndex::TransferToAccountIndex => {
                // args(0) = pointer to array of bytes of an account hash
                // args(1) = length of array of bytes of an account hash
                // args(2) = pointer to array of bytes of an amount
                // args(3) = length of array of bytes of an amount
                // args(4) = pointer to array of bytes of an id
                // args(5) = length of array of bytes of an id
                // args(6) = pointer to a value where new value will be set
                let (key_ptr, key_size, amount_ptr, amount_size, id_ptr, id_size, result_ptr) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.transfer_to_account,
                    [
                        key_ptr,
                        key_size,
                        amount_ptr,
                        amount_size,
                        id_ptr,
                        id_size,
                        result_ptr,
                    ],
                )?;
                let account_hash: AccountHash = {
                    let bytes = self.bytes_from_mem(key_ptr, key_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };
                let amount: U512 = {
                    let bytes = self.bytes_from_mem(amount_ptr, amount_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };
                let id: Option<u64> = {
                    let bytes = self.bytes_from_mem(id_ptr, id_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };

                let ret = match self.transfer_to_account(account_hash, amount, id)? {
                    Ok(transferred_to) => {
                        let result_value: u32 = transferred_to as u32;
                        let result_value_bytes = result_value.to_le_bytes();
                        self.try_get_memory()?
                            .set(result_ptr, &result_value_bytes)
                            .map_err(|error| ExecError::Interpreter(error.into()))?;
                        Ok(())
                    }
                    Err(api_error) => Err(api_error),
                };
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::TransferFromPurseToAccountIndex => {
                // args(0) = pointer to array of bytes in Wasm memory of a source purse
                // args(1) = length of array of bytes in Wasm memory of a source purse
                // args(2) = pointer to array of bytes in Wasm memory of an account hash
                // args(3) = length of array of bytes in Wasm memory of an account hash
                // args(4) = pointer to array of bytes in Wasm memory of an amount
                // args(5) = length of array of bytes in Wasm memory of an amount
                // args(6) = pointer to array of bytes in Wasm memory of an id
                // args(7) = length of array of bytes in Wasm memory of an id
                // args(8) = pointer to a value where value of `TransferredTo` enum will be set
                let (
                    source_ptr,
                    source_size,
                    key_ptr,
                    key_size,
                    amount_ptr,
                    amount_size,
                    id_ptr,
                    id_size,
                    result_ptr,
                ) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.transfer_from_purse_to_account,
                    [
                        source_ptr,
                        source_size,
                        key_ptr,
                        key_size,
                        amount_ptr,
                        amount_size,
                        id_ptr,
                        id_size,
                        result_ptr,
                    ],
                )?;
                let source_purse = {
                    let bytes = self.bytes_from_mem(source_ptr, source_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };
                let account_hash: AccountHash = {
                    let bytes = self.bytes_from_mem(key_ptr, key_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };
                let amount: U512 = {
                    let bytes = self.bytes_from_mem(amount_ptr, amount_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };
                let id: Option<u64> = {
                    let bytes = self.bytes_from_mem(id_ptr, id_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };
                let ret = match self.transfer_from_purse_to_account_hash(
                    source_purse,
                    account_hash,
                    amount,
                    id,
                )? {
                    Ok(transferred_to) => {
                        let result_value: u32 = transferred_to as u32;
                        let result_value_bytes = result_value.to_le_bytes();
                        self.try_get_memory()?
                            .set(result_ptr, &result_value_bytes)
                            .map_err(|error| ExecError::Interpreter(error.into()))?;
                        Ok(())
                    }
                    Err(api_error) => Err(api_error),
                };
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::TransferFromPurseToPurseIndex => {
                // args(0) = pointer to array of bytes in Wasm memory of a source purse
                // args(1) = length of array of bytes in Wasm memory of a source purse
                // args(2) = pointer to array of bytes in Wasm memory of a target purse
                // args(3) = length of array of bytes in Wasm memory of a target purse
                // args(4) = pointer to array of bytes in Wasm memory of an amount
                // args(5) = length of array of bytes in Wasm memory of an amount
                // args(6) = pointer to array of bytes in Wasm memory of an id
                // args(7) = length of array of bytes in Wasm memory of an id
                let (
                    source_ptr,
                    source_size,
                    target_ptr,
                    target_size,
                    amount_ptr,
                    amount_size,
                    id_ptr,
                    id_size,
                ) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.transfer_from_purse_to_purse,
                    [
                        source_ptr,
                        source_size,
                        target_ptr,
                        target_size,
                        amount_ptr,
                        amount_size,
                        id_ptr,
                        id_size,
                    ],
                )?;

                let source: URef = {
                    let bytes = self.bytes_from_mem(source_ptr, source_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };

                let target: URef = {
                    let bytes = self.bytes_from_mem(target_ptr, target_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };

                let amount: U512 = {
                    let bytes = self.bytes_from_mem(amount_ptr, amount_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };

                let id: Option<u64> = {
                    let bytes = self.bytes_from_mem(id_ptr, id_size as usize)?;
                    bytesrepr::deserialize_from_slice(bytes).map_err(ExecError::BytesRepr)?
                };

                let ret = self.transfer_from_purse_to_purse(source, target, amount, id)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::GetBalanceIndex => {
                // args(0) = pointer to purse input
                // args(1) = length of purse
                // args(2) = pointer to output size (output)
                let (ptr, ptr_size, output_size_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.get_balance,
                    [ptr, ptr_size, output_size_ptr],
                )?;
                let ret = self.get_balance_host_buffer(ptr, ptr_size as usize, output_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::GetPhaseIndex => {
                // args(0) = pointer to Wasm memory where to write.
                let (dest_ptr,) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.get_phase, [dest_ptr])?;
                self.get_phase(dest_ptr)?;
                Ok(None)
            }

            FunctionIndex::GetSystemContractIndex => {
                // args(0) = system contract index
                // args(1) = dest pointer for storing serialized result
                // args(2) = dest pointer size
                let (system_contract_index, dest_ptr, dest_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.get_system_contract,
                    [system_contract_index, dest_ptr, dest_size],
                )?;
                let ret = self.get_system_contract(system_contract_index, dest_ptr, dest_size)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::GetMainPurseIndex => {
                // args(0) = pointer to Wasm memory where to write.
                let (dest_ptr,) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.get_main_purse, [dest_ptr])?;
                self.get_main_purse(dest_ptr)?;
                Ok(None)
            }

            FunctionIndex::ReadHostBufferIndex => {
                // args(0) = pointer to Wasm memory where to write size.
                let (dest_ptr, dest_size, bytes_written_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.read_host_buffer,
                    [dest_ptr, dest_size, bytes_written_ptr],
                )?;
                let ret = self.read_host_buffer(dest_ptr, dest_size as usize, bytes_written_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::CreateContractPackageAtHash => {
                // args(0) = pointer to wasm memory where to write 32-byte Hash address
                // args(1) = pointer to wasm memory where to write 32-byte access key address
                // args(2) = boolean flag to determine if the contract can be versioned
                let (hash_dest_ptr, access_dest_ptr, is_locked) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.create_contract_package_at_hash,
                    [hash_dest_ptr, access_dest_ptr],
                )?;
                let package_status = PackageStatus::new(is_locked);
                let (hash_addr, access_addr) =
                    self.create_contract_package_at_hash(package_status)?;

                self.function_address(hash_addr, hash_dest_ptr)?;
                self.function_address(access_addr, access_dest_ptr)?;
                Ok(None)
            }

            FunctionIndex::CreateContractUserGroup => {
                // args(0) = pointer to package key in wasm memory
                // args(1) = size of package key in wasm memory
                // args(2) = pointer to group label in wasm memory
                // args(3) = size of group label in wasm memory
                // args(4) = number of new urefs to generate for the group
                // args(5) = pointer to existing_urefs in wasm memory
                // args(6) = size of existing_urefs in wasm memory
                // args(7) = pointer to location to write size of output (written to host buffer)
                let (
                    package_key_ptr,
                    package_key_size,
                    label_ptr,
                    label_size,
                    num_new_urefs,
                    existing_urefs_ptr,
                    existing_urefs_size,
                    output_size_ptr,
                ) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.create_contract_user_group,
                    [
                        package_key_ptr,
                        package_key_size,
                        label_ptr,
                        label_size,
                        num_new_urefs,
                        existing_urefs_ptr,
                        existing_urefs_size,
                        output_size_ptr,
                    ],
                )?;

                let contract_package_hash: PackageHash =
                    self.t_from_mem(package_key_ptr, package_key_size)?;
                let label: String = self.t_from_mem(label_ptr, label_size)?;
                let existing_urefs: BTreeSet<URef> =
                    self.t_from_mem(existing_urefs_ptr, existing_urefs_size)?;

                let ret = self.create_contract_user_group(
                    contract_package_hash,
                    label,
                    num_new_urefs,
                    existing_urefs,
                    output_size_ptr,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }
            FunctionIndex::AddPackageVersion => {
                // args(0)  = pointer to package hash in wasm memory
                // args(1)  = size of package hash in wasm memory
                // args(2)  = pointer to entity version in wasm memory
                // args(3)  = pointer to entrypoints in wasm memory
                // args(4)  = size of entrypoints in wasm memory
                // args(5)  = pointer to named keys in wasm memory
                // args(6)  = size of named keys in wasm memory
                // args(7)  = pointer to the new topic names in wasm memory
                // args(8)  = size of the new topic names in wasm memory
                // args(9)  = pointer to output buffer for serialized key
                // args(10) = size of output buffer
                let (
                    contract_package_hash_ptr,
                    contract_package_hash_size,
                    version_ptr,
                    entry_points_ptr,
                    entry_points_size,
                    named_keys_ptr,
                    named_keys_size,
                    message_topics,
                    message_topics_size,
                    output_ptr,
                    output_size,
                ) = Args::parse(args)?;

                self.charge_host_function_call(
                    &host_function_costs.add_contract_version,
                    [
                        contract_package_hash_ptr,
                        contract_package_hash_size,
                        version_ptr,
                        entry_points_ptr,
                        entry_points_size,
                        named_keys_ptr,
                        named_keys_size,
                        message_topics,
                        message_topics_size,
                        output_ptr,
                        output_size,
                    ],
                )?;

                // Exit if unable to return output.
                if output_size < 32 {
                    // `output_size` must be >= actual length of serialized hash bytes
                    return Ok(Some(RuntimeValue::I32(api_error::i32_from(Err(
                        ApiError::BufferTooSmall,
                    )))));
                }

                let package_hash: PackageHash =
                    self.t_from_mem(contract_package_hash_ptr, contract_package_hash_size)?;
                let entry_points: EntryPoints =
                    self.t_from_mem(entry_points_ptr, entry_points_size)?;
                let named_keys: NamedKeys = self.t_from_mem(named_keys_ptr, named_keys_size)?;
                let message_topics: BTreeMap<String, MessageTopicOperation> =
                    self.t_from_mem(message_topics, message_topics_size)?;

                // Check that the names of the topics that are added are within the configured
                // limits.
                let message_limits = self.context.engine_config().wasm_config().messages_limits();
                for (topic_name, _) in
                    message_topics
                        .iter()
                        .filter(|(_, operation)| match operation {
                            MessageTopicOperation::Add => true,
                        })
                {
                    if topic_name.len() > message_limits.max_topic_name_size() as usize {
                        return Ok(Some(RuntimeValue::I32(api_error::i32_from(Err(
                            ApiError::MaxTopicNameSizeExceeded,
                        )))));
                    }
                }

                let ret = self.add_contract_version(
                    package_hash,
                    version_ptr,
                    entry_points,
                    named_keys,
                    message_topics,
                    output_ptr,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::DisableContractVersion => {
                // args(0) = pointer to package hash in wasm memory
                // args(1) = size of package hash in wasm memory
                // args(2) = pointer to contract hash in wasm memory
                // args(3) = size of contract hash in wasm memory
                let (package_key_ptr, package_key_size, contract_hash_ptr, contract_hash_size) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.disable_contract_version,
                    [
                        package_key_ptr,
                        package_key_size,
                        contract_hash_ptr,
                        contract_hash_size,
                    ],
                )?;
                let contract_package_hash = self.t_from_mem(package_key_ptr, package_key_size)?;
                let contract_hash = self.t_from_mem(contract_hash_ptr, contract_hash_size)?;

                let result = self.disable_contract_version(contract_package_hash, contract_hash)?;

                Ok(Some(RuntimeValue::I32(api_error::i32_from(result))))
            }

            FunctionIndex::CallContractFuncIndex => {
                // args(0) = pointer to contract hash where contract is at in global state
                // args(1) = size of contract hash
                // args(2) = pointer to entry point
                // args(3) = size of entry point
                // args(4) = pointer to function arguments in Wasm memory
                // args(5) = size of arguments
                // args(6) = pointer to result size (output)
                let (
                    contract_hash_ptr,
                    contract_hash_size,
                    entry_point_name_ptr,
                    entry_point_name_size,
                    args_ptr,
                    args_size,
                    result_size_ptr,
                ) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.call_contract,
                    [
                        contract_hash_ptr,
                        contract_hash_size,
                        entry_point_name_ptr,
                        entry_point_name_size,
                        args_ptr,
                        args_size,
                        result_size_ptr,
                    ],
                )?;

                let contract_hash: AddressableEntityHash =
                    self.t_from_mem(contract_hash_ptr, contract_hash_size)?;
                let entry_point_name: String =
                    self.t_from_mem(entry_point_name_ptr, entry_point_name_size)?;
                let args_bytes: Vec<u8> = {
                    let args_size: u32 = args_size;
                    self.bytes_from_mem(args_ptr, args_size as usize)?.to_vec()
                };

                let ret = self.call_contract_host_buffer(
                    contract_hash,
                    &entry_point_name,
                    &args_bytes,
                    result_size_ptr,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::CallVersionedContract => {
                // args(0) = pointer to contract_package_hash where contract is at in global state
                // args(1) = size of contract_package_hash
                // args(2) = pointer to contract version in wasm memory
                // args(3) = size of contract version in wasm memory
                // args(4) = pointer to method name in wasm memory
                // args(5) = size of method name in wasm memory
                // args(6) = pointer to function arguments in Wasm memory
                // args(7) = size of arguments
                // args(8) = pointer to result size (output)
                let (
                    contract_package_hash_ptr,
                    contract_package_hash_size,
                    contract_version_ptr,
                    contract_package_size,
                    entry_point_name_ptr,
                    entry_point_name_size,
                    args_ptr,
                    args_size,
                    result_size_ptr,
                ) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.call_versioned_contract,
                    [
                        contract_package_hash_ptr,
                        contract_package_hash_size,
                        contract_version_ptr,
                        contract_package_size,
                        entry_point_name_ptr,
                        entry_point_name_size,
                        args_ptr,
                        args_size,
                        result_size_ptr,
                    ],
                )?;

                let contract_package_hash: PackageHash =
                    self.t_from_mem(contract_package_hash_ptr, contract_package_hash_size)?;
                let contract_version: Option<EntityVersion> =
                    self.t_from_mem(contract_version_ptr, contract_package_size)?;
                let entry_point_name: String =
                    self.t_from_mem(entry_point_name_ptr, entry_point_name_size)?;
                let args_bytes: Vec<u8> = {
                    let args_size: u32 = args_size;
                    self.bytes_from_mem(args_ptr, args_size as usize)?.to_vec()
                };

                let ret = self.call_versioned_contract_host_buffer(
                    contract_package_hash,
                    contract_version,
                    entry_point_name,
                    &args_bytes,
                    result_size_ptr,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            #[cfg(feature = "test-support")]
            FunctionIndex::PrintIndex => {
                let (text_ptr, text_size) = Args::parse(args)?;
                self.charge_host_function_call(&host_function_costs.print, [text_ptr, text_size])?;
                self.print(text_ptr, text_size)?;
                Ok(None)
            }

            FunctionIndex::GetRuntimeArgsizeIndex => {
                // args(0) = pointer to name of host runtime arg to load
                // args(1) = size of name of the host runtime arg
                // args(2) = pointer to a argument size (output)
                let (name_ptr, name_size, size_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.get_named_arg_size,
                    [name_ptr, name_size, size_ptr],
                )?;
                let ret = self.get_named_arg_size(name_ptr, name_size as usize, size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::GetRuntimeArgIndex => {
                // args(0) = pointer to serialized argument name
                // args(1) = size of serialized argument name
                // args(2) = pointer to output pointer where host will write argument bytes
                // args(3) = size of available data under output pointer
                let (name_ptr, name_size, dest_ptr, dest_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.get_named_arg,
                    [name_ptr, name_size, dest_ptr, dest_size],
                )?;
                let ret =
                    self.get_named_arg(name_ptr, name_size as usize, dest_ptr, dest_size as usize)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::RemoveContractUserGroupIndex => {
                // args(0) = pointer to package key in wasm memory
                // args(1) = size of package key in wasm memory
                // args(2) = pointer to serialized group label
                // args(3) = size of serialized group label
                let (package_key_ptr, package_key_size, label_ptr, label_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.remove_contract_user_group,
                    [package_key_ptr, package_key_size, label_ptr, label_size],
                )?;
                let package_key = self.t_from_mem(package_key_ptr, package_key_size)?;
                let label: Group = self.t_from_mem(label_ptr, label_size)?;

                let ret = self.remove_contract_user_group(package_key, label)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::ExtendContractUserGroupURefsIndex => {
                // args(0) = pointer to package key in wasm memory
                // args(1) = size of package key in wasm memory
                // args(2) = pointer to label name
                // args(3) = label size bytes
                // args(4) = output of size value of host bytes data
                let (package_ptr, package_size, label_ptr, label_size, value_size_ptr) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.provision_contract_user_group_uref,
                    [
                        package_ptr,
                        package_size,
                        label_ptr,
                        label_size,
                        value_size_ptr,
                    ],
                )?;
                let ret = self.provision_contract_user_group_uref(
                    package_ptr,
                    package_size,
                    label_ptr,
                    label_size,
                    value_size_ptr,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::RemoveContractUserGroupURefsIndex => {
                // args(0) = pointer to package key in wasm memory
                // args(1) = size of package key in wasm memory
                // args(2) = pointer to label name
                // args(3) = label size bytes
                // args(4) = pointer to urefs
                // args(5) = size of urefs pointer
                let (package_ptr, package_size, label_ptr, label_size, urefs_ptr, urefs_size) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.remove_contract_user_group_urefs,
                    [
                        package_ptr,
                        package_size,
                        label_ptr,
                        label_size,
                        urefs_ptr,
                        urefs_size,
                    ],
                )?;
                let ret = self.remove_contract_user_group_urefs(
                    package_ptr,
                    package_size,
                    label_ptr,
                    label_size,
                    urefs_ptr,
                    urefs_size,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::Blake2b => {
                let (in_ptr, in_size, out_ptr, out_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.blake2b,
                    [in_ptr, in_size, out_ptr, out_size],
                )?;
                let digest =
                    self.checked_memory_slice(in_ptr as usize, in_size as usize, |input| {
                        crypto::blake2b(input)
                    })?;

                let result = if digest.len() != out_size as usize {
                    Err(ApiError::BufferTooSmall)
                } else {
                    Ok(())
                };
                if result.is_err() {
                    return Ok(Some(RuntimeValue::I32(api_error::i32_from(result))));
                }

                self.try_get_memory()?
                    .set(out_ptr, &digest)
                    .map_err(|error| ExecError::Interpreter(error.into()))?;
                Ok(Some(RuntimeValue::I32(0)))
            }

            FunctionIndex::NewDictionaryFuncIndex => {
                // args(0) = pointer to output size (output param)
                let (output_size_ptr,): (u32,) = Args::parse(args)?;

                self.charge_host_function_call(
                    &DEFAULT_HOST_FUNCTION_NEW_DICTIONARY,
                    [output_size_ptr],
                )?;
                let ret = self.new_dictionary(output_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::DictionaryGetFuncIndex => {
                // args(0) = pointer to uref in Wasm memory
                // args(1) = size of uref in Wasm memory
                // args(2) = pointer to key bytes pointer in Wasm memory
                // args(3) = pointer to key bytes size in Wasm memory
                // args(4) = pointer to output size (output param)
                let (uref_ptr, uref_size, key_bytes_ptr, key_bytes_size, output_size_ptr): (
                    _,
                    u32,
                    _,
                    u32,
                    _,
                ) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.dictionary_get,
                    [key_bytes_ptr, key_bytes_size, output_size_ptr],
                )?;
                let ret = self.dictionary_get(
                    uref_ptr,
                    uref_size,
                    key_bytes_ptr,
                    key_bytes_size,
                    output_size_ptr,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::DictionaryPutFuncIndex => {
                // args(0) = pointer to uref in Wasm memory
                // args(1) = size of uref in Wasm memory
                // args(2) = pointer to key bytes pointer in Wasm memory
                // args(3) = pointer to key bytes size in Wasm memory
                // args(4) = pointer to value bytes pointer in Wasm memory
                // args(5) = pointer to value bytes size in Wasm memory
                let (uref_ptr, uref_size, key_bytes_ptr, key_bytes_size, value_ptr, value_ptr_size): (_, u32, _, u32, _, u32) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.dictionary_put,
                    [key_bytes_ptr, key_bytes_size, value_ptr, value_ptr_size],
                )?;
                let ret = self.dictionary_put(
                    uref_ptr,
                    uref_size,
                    key_bytes_ptr,
                    key_bytes_size,
                    value_ptr,
                    value_ptr_size,
                )?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::DictionaryReadFuncIndex => {
                // args(0) = pointer to key in Wasm memory
                // args(1) = size of key in Wasm memory
                // args(2) = pointer to output size (output param)
                let (key_ptr, key_size, output_size_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.read_value,
                    [key_ptr, key_size, output_size_ptr],
                )?;
                let ret = self.dictionary_read(key_ptr, key_size, output_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::LoadCallStack => {
                // args(0) (Output) Pointer to number of elements in the call stack.
                // args(1) (Output) Pointer to size in bytes of the serialized call stack.
                let (call_stack_len_ptr, result_size_ptr) = Args::parse(args)?;
                // TODO: add cost table entry once we can upgrade safely
                self.charge_host_function_call(
                    &HostFunction::fixed(10_000),
                    [call_stack_len_ptr, result_size_ptr],
                )?;
                let ret = self.load_call_stack(call_stack_len_ptr, result_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::LoadAuthorizationKeys => {
                // args(0) (Output) Pointer to number of authorization keys.
                // args(1) (Output) Pointer to size in bytes of the total bytes.
                let (len_ptr, result_size_ptr) = Args::parse(args)?;
                self.charge_host_function_call(
                    &HostFunction::fixed(10_000),
                    [len_ptr, result_size_ptr],
                )?;
                let ret = self.load_authorization_keys(len_ptr, result_size_ptr)?;
                Ok(Some(RuntimeValue::I32(api_error::i32_from(ret))))
            }

            FunctionIndex::RandomBytes => {
                let (out_ptr, out_size) = Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.random_bytes,
                    [out_ptr, out_size],
                )?;

                let random_bytes = self.context.random_bytes()?;

                let result = if random_bytes.len() != out_size as usize {
                    Err(ApiError::BufferTooSmall)
                } else {
                    Ok(())
                };
                if result.is_err() {
                    return Ok(Some(RuntimeValue::I32(api_error::i32_from(result))));
                }

                self.try_get_memory()?
                    .set(out_ptr, &random_bytes)
                    .map_err(|error| ExecError::Interpreter(error.into()))?;

                Ok(Some(RuntimeValue::I32(0)))
            }
            FunctionIndex::EnableContractVersion => {
                // args(0) = pointer to package hash in wasm memory
                // args(1) = size of package hash in wasm memory
                // args(2) = pointer to contract hash in wasm memory
                // args(3) = size of contract hash in wasm memory
                let (package_key_ptr, package_key_size, contract_hash_ptr, contract_hash_size) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.enable_contract_version,
                    [
                        package_key_ptr,
                        package_key_size,
                        contract_hash_ptr,
                        contract_hash_size,
                    ],
                )?;
                let contract_package_hash = self.t_from_mem(package_key_ptr, package_key_size)?;
                let contract_hash = self.t_from_mem(contract_hash_ptr, contract_hash_size)?;

                let result = self.enable_contract_version(contract_package_hash, contract_hash)?;

                Ok(Some(RuntimeValue::I32(api_error::i32_from(result))))
            }
            FunctionIndex::ManageMessageTopic => {
                // args(0) = pointer to the serialized topic name string in wasm memory
                // args(1) = size of the serialized topic name string in wasm memory
                // args(2) = pointer to the operation to be performed for the specified topic
                // args(3) = size of the operation
                let (topic_name_ptr, topic_name_size, operation_ptr, operation_size) =
                    Args::parse(args)?;
                self.charge_host_function_call(
                    &host_function_costs.manage_message_topic,
                    [
                        topic_name_ptr,
                        topic_name_size,
                        operation_ptr,
                        operation_size,
                    ],
                )?;

                let limits = self.context.engine_config().wasm_config().messages_limits();

                if topic_name_size > limits.max_topic_name_size() {
                    return Ok(Some(RuntimeValue::I32(api_error::i32_from(Err(
                        ApiError::MaxTopicNameSizeExceeded,
                    )))));
                }

                let topic_name_bytes =
                    self.bytes_from_mem(topic_name_ptr, topic_name_size as usize)?;
                let topic_name = std::str::from_utf8(&topic_name_bytes)
                    .map_err(|e| Trap::from(ExecError::InvalidUtf8Encoding(e)))?;

                if operation_size as usize > MessageTopicOperation::max_serialized_len() {
                    return Err(Trap::from(ExecError::InvalidMessageTopicOperation));
                }
                let topic_operation = self
                    .t_from_mem(operation_ptr, operation_size)
                    .map_err(|_e| Trap::from(ExecError::InvalidMessageTopicOperation))?;

                // only allow managing messages from stored contracts
                if !self.context.get_entity_key().is_smart_contract_key() {
                    return Err(Trap::from(ExecError::InvalidContext));
                }

                let result = match topic_operation {
                    MessageTopicOperation::Add => {
                        self.add_message_topic(topic_name).map_err(Trap::from)?
                    }
                };

                Ok(Some(RuntimeValue::I32(api_error::i32_from(result))))
            }
            FunctionIndex::EmitMessage => {
                // args(0) = pointer to the serialized topic name string in wasm memory
                // args(1) = size of the serialized name string in wasm memory
                // args(2) = pointer to the serialized message payload in wasm memory
                // args(3) = size of the serialized message payload in wasm memory
                let (topic_name_ptr, topic_name_size, message_ptr, message_size) =
                    Args::parse(args)?;

                // Charge for the call to emit message. This increases for every message emitted
                // within an execution so we're not using the static value from the wasm config.
                self.context
                    .charge_gas(Gas::new(self.context.emit_message_cost()))?;
                // Charge for parameter weights.
                self.charge_host_function_call(
                    &HostFunction::new(0, host_function_costs.emit_message.arguments()),
                    &[topic_name_ptr, topic_name_size, message_ptr, message_size],
                )?;

                let limits = self.context.engine_config().wasm_config().messages_limits();

                if topic_name_size > limits.max_topic_name_size() {
                    return Ok(Some(RuntimeValue::I32(api_error::i32_from(Err(
                        ApiError::MaxTopicNameSizeExceeded,
                    )))));
                }

                if message_size > limits.max_message_size() {
                    return Ok(Some(RuntimeValue::I32(api_error::i32_from(Err(
                        ApiError::MessageTooLarge,
                    )))));
                }

                let topic_name_bytes =
                    self.bytes_from_mem(topic_name_ptr, topic_name_size as usize)?;
                let topic_name = std::str::from_utf8(&topic_name_bytes)
                    .map_err(|e| Trap::from(ExecError::InvalidUtf8Encoding(e)))?;

                let message = self.t_from_mem(message_ptr, message_size)?;

                let result = self.emit_message(topic_name, message)?;
                if result.is_ok() {
                    // Increase the cost for the next call to emit a message.
                    let new_cost = self
                        .context
                        .emit_message_cost()
                        .checked_add(host_function_costs.cost_increase_per_message.into())
                        .ok_or(ExecError::GasLimit)?;
                    self.context.set_emit_message_cost(new_cost);
                }
                Ok(Some(RuntimeValue::I32(api_error::i32_from(result))))
            }
        }
    }
}
