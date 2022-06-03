use std::cell::RefCell;

use wasmi::{
    memory_units::Pages, Error as InterpreterError, FuncInstance, FuncRef, MemoryDescriptor,
    MemoryInstance, MemoryRef, ModuleImportResolver, Signature, ValueType,
};

use super::{
    error::ResolverError, memory_resolver::MemoryResolver, v1_function_index::FunctionIndex,
};

pub(crate) struct RuntimeModuleImportResolver {
    memory: RefCell<Option<MemoryRef>>,
    max_memory: u32,
}

impl RuntimeModuleImportResolver {
    pub(crate) fn new(max_memory: u32) -> Self {
        Self {
            memory: RefCell::new(None),
            max_memory,
        }
    }
}

impl MemoryResolver for RuntimeModuleImportResolver {
    fn memory_ref(&self) -> Result<MemoryRef, ResolverError> {
        self.memory
            .borrow()
            .as_ref()
            .map(Clone::clone)
            .ok_or(ResolverError::NoImportedMemory)
    }
}

impl ModuleImportResolver for RuntimeModuleImportResolver {
    fn resolve_func(
        &self,
        field_name: &str,
        _signature: &Signature,
    ) -> Result<FuncRef, InterpreterError> {
        let func_ref = match field_name {
            "casper_read_value" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::ReadFuncIndex.into(),
            ),
            "casper_load_named_keys" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::LoadNamedKeysFuncIndex.into(),
            ),
            "casper_write" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], None),
                FunctionIndex::WriteFuncIndex.into(),
            ),
            "casper_add" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], None),
                FunctionIndex::AddFuncIndex.into(),
            ),
            "casper_new_uref" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], None),
                FunctionIndex::NewFuncIndex.into(),
            ),
            "casper_ret" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::RetFuncIndex.into(),
            ),
            "casper_get_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::GetKeyFuncIndex.into(),
            ),
            "casper_has_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::HasKeyFuncIndex.into(),
            ),
            "casper_put_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], None),
                FunctionIndex::PutKeyFuncIndex.into(),
            ),
            "gas" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GasFuncIndex.into(),
            ),
            "casper_is_valid_uref" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::IsValidURefFnIndex.into(),
            ),
            "casper_revert" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::RevertFuncIndex.into(),
            ),
            "casper_add_associated_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::AddAssociatedKeyFuncIndex.into(),
            ),
            "casper_remove_associated_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::RemoveAssociatedKeyFuncIndex.into(),
            ),
            "casper_update_associated_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::UpdateAssociatedKeyFuncIndex.into(),
            ),
            "casper_set_action_threshold" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::SetActionThresholdFuncIndex.into(),
            ),
            "casper_remove_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::RemoveKeyFuncIndex.into(),
            ),
            "casper_get_caller" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], Some(ValueType::I32)),
                FunctionIndex::GetCallerIndex.into(),
            ),
            "casper_get_blocktime" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GetBlocktimeIndex.into(),
            ),
            "casper_create_purse" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::CreatePurseIndex.into(),
            ),
            "casper_transfer_to_account" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 7][..], Some(ValueType::I32)),
                FunctionIndex::TransferToAccountIndex.into(),
            ),
            "casper_transfer_from_purse_to_account" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 9][..], Some(ValueType::I32)),
                FunctionIndex::TransferFromPurseToAccountIndex.into(),
            ),
            "casper_transfer_from_purse_to_purse" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 8][..], Some(ValueType::I32)),
                FunctionIndex::TransferFromPurseToPurseIndex.into(),
            ),
            "casper_get_balance" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::GetBalanceIndex.into(),
            ),
            "casper_get_phase" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GetPhaseIndex.into(),
            ),
            "casper_get_system_contract" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::GetSystemContractIndex.into(),
            ),
            "casper_get_main_purse" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GetMainPurseIndex.into(),
            ),
            "casper_read_host_buffer" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::ReadHostBufferIndex.into(),
            ),
            "casper_create_contract_package_at_hash" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], None),
                FunctionIndex::CreateContractPackageAtHash.into(),
            ),
            "casper_create_contract_user_group" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 8][..], Some(ValueType::I32)),
                FunctionIndex::CreateContractUserGroup.into(),
            ),
            "casper_add_contract_version" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 10][..], Some(ValueType::I32)),
                FunctionIndex::AddContractVersion.into(),
            ),
            "casper_disable_contract_version" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::DisableContractVersion.into(),
            ),
            "casper_call_contract" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 7][..], Some(ValueType::I32)),
                FunctionIndex::CallContractFuncIndex.into(),
            ),
            "casper_call_versioned_contract" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 9][..], Some(ValueType::I32)),
                FunctionIndex::CallVersionedContract.into(),
            ),
            "casper_get_named_arg_size" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::GetRuntimeArgsizeIndex.into(),
            ),
            "casper_get_named_arg" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::GetRuntimeArgIndex.into(),
            ),
            "casper_remove_contract_user_group" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::RemoveContractUserGroupIndex.into(),
            ),
            "casper_provision_contract_user_group_uref" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::ExtendContractUserGroupURefsIndex.into(),
            ),
            "casper_remove_contract_user_group_urefs" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 6][..], Some(ValueType::I32)),
                FunctionIndex::RemoveContractUserGroupURefsIndex.into(),
            ),
            "casper_blake2b" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::Blake2b.into(),
            ),
            "casper_record_transfer" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 10][..], Some(ValueType::I32)),
                FunctionIndex::RecordTransfer.into(),
            ),
            "casper_record_era_info" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::RecordEraInfo.into(),
            ),
            "casper_load_call_stack" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::LoadCallStack.into(),
            ),
            #[cfg(feature = "test-support")]
            "casper_print" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::PrintIndex.into(),
            ),
            "casper_dictionary_get" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::DictionaryGetFuncIndex.into(),
            ),
            "casper_dictionary_put" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 6][..], Some(ValueType::I32)),
                FunctionIndex::DictionaryPutFuncIndex.into(),
            ),
            "casper_new_dictionary" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], Some(ValueType::I32)),
                FunctionIndex::NewDictionaryFuncIndex.into(),
            ),
            "casper_load_authorization_keys" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::LoadAuthorizationKeys.into(),
            ),
            "casper_random_bytes" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::RandomBytes.into(),
            ),
            _ => {
                return Err(InterpreterError::Function(format!(
                    "host module doesn't export function with name {}",
                    field_name
                )));
            }
        };
        Ok(func_ref)
    }

    fn resolve_memory(
        &self,
        field_name: &str,
        descriptor: &MemoryDescriptor,
    ) -> Result<MemoryRef, InterpreterError> {
        if field_name == "memory" {
            match &mut *self.memory.borrow_mut() {
                Some(_) => {
                    // Even though most wat -> wasm compilers don't allow multiple memory entries,
                    // we should make sure we won't accidentally allocate twice.
                    Err(InterpreterError::Instantiation(
                        "Memory is already instantiated".into(),
                    ))
                }
                memory_ref @ None => {
                    // Any memory entry in the wasm file without max specified is changed into an
                    // entry with hardcoded max value. This way `maximum` below is never
                    // unspecified, but for safety reasons we'll still default it.
                    let descriptor_max = descriptor.maximum().unwrap_or(self.max_memory);
                    // Checks if wasm's memory entry has too much initial memory or non-default max
                    // memory pages exceeds the limit.
                    if descriptor.initial() > descriptor_max || descriptor_max > self.max_memory {
                        return Err(InterpreterError::Instantiation(
                            "Module requested too much memory".into(),
                        ));
                    }
                    // Note: each "page" is 64 KiB
                    let mem = MemoryInstance::alloc(
                        Pages(descriptor.initial() as usize),
                        descriptor.maximum().map(|x| Pages(x as usize)),
                    )?;
                    *memory_ref = Some(mem.clone());
                    Ok(mem)
                }
            }
        } else {
            Err(InterpreterError::Instantiation(
                "Memory imported under unknown name".to_owned(),
            ))
        }
    }
}
