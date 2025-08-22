use std::cell::RefCell;

use casper_wasmi::{
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
                FunctionIndex::ReadFuncIndex as usize,
            ),
            "casper_load_named_keys" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::LoadNamedKeysFuncIndex as usize,
            ),
            "casper_write" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], None),
                FunctionIndex::WriteFuncIndex as usize,
            ),
            "casper_add" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], None),
                FunctionIndex::AddFuncIndex as usize,
            ),
            "casper_new_uref" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], None),
                FunctionIndex::NewFuncIndex as usize,
            ),
            "casper_ret" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::RetFuncIndex as usize,
            ),
            "casper_get_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::GetKeyFuncIndex as usize,
            ),
            "casper_has_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::HasKeyFuncIndex as usize,
            ),
            "casper_put_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], None),
                FunctionIndex::PutKeyFuncIndex as usize,
            ),
            "gas" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GasFuncIndex as usize,
            ),
            "casper_is_valid_uref" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::IsValidURefFnIndex as usize,
            ),
            "casper_revert" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::RevertFuncIndex as usize,
            ),
            "casper_add_associated_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::AddAssociatedKeyFuncIndex as usize,
            ),
            "casper_remove_associated_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::RemoveAssociatedKeyFuncIndex as usize,
            ),
            "casper_update_associated_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::UpdateAssociatedKeyFuncIndex as usize,
            ),
            "casper_set_action_threshold" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::SetActionThresholdFuncIndex as usize,
            ),
            "casper_remove_key" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::RemoveKeyFuncIndex as usize,
            ),
            "casper_get_caller" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], Some(ValueType::I32)),
                FunctionIndex::GetCallerIndex as usize,
            ),
            "casper_get_blocktime" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GetBlocktimeIndex as usize,
            ),
            "casper_create_purse" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::CreatePurseIndex as usize,
            ),
            "casper_transfer_to_account" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 7][..], Some(ValueType::I32)),
                FunctionIndex::TransferToAccountIndex as usize,
            ),
            "casper_transfer_from_purse_to_account" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 9][..], Some(ValueType::I32)),
                FunctionIndex::TransferFromPurseToAccountIndex as usize,
            ),
            "casper_transfer_from_purse_to_purse" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 8][..], Some(ValueType::I32)),
                FunctionIndex::TransferFromPurseToPurseIndex as usize,
            ),
            "casper_get_balance" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::GetBalanceIndex as usize,
            ),
            "casper_get_phase" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GetPhaseIndex as usize,
            ),
            "casper_get_system_contract" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::GetSystemContractIndex as usize,
            ),
            "casper_get_main_purse" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], None),
                FunctionIndex::GetMainPurseIndex as usize,
            ),
            "casper_read_host_buffer" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::ReadHostBufferIndex as usize,
            ),
            "casper_create_contract_package_at_hash" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], None),
                FunctionIndex::CreateContractPackageAtHash as usize,
            ),
            "casper_create_contract_user_group" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 8][..], Some(ValueType::I32)),
                FunctionIndex::CreateContractUserGroup as usize,
            ),
            "casper_add_contract_version" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 10][..], Some(ValueType::I32)),
                FunctionIndex::AddContractVersion as usize,
            ),
            "casper_add_contract_version_with_message_topics" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 11][..], Some(ValueType::I32)),
                FunctionIndex::AddContractVersionWithMessageTopics as usize,
            ),
            "casper_add_package_version_with_message_topics" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 11][..], Some(ValueType::I32)),
                FunctionIndex::AddPackageVersionWithMessageTopics as usize,
            ),
            "casper_disable_contract_version" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::DisableContractVersion as usize,
            ),
            "casper_call_contract" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 7][..], Some(ValueType::I32)),
                FunctionIndex::CallContractFuncIndex as usize,
            ),
            "casper_call_versioned_contract" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 9][..], Some(ValueType::I32)),
                FunctionIndex::CallVersionedContract as usize,
            ),
            "casper_get_named_arg_size" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::GetRuntimeArgsizeIndex as usize,
            ),
            "casper_get_named_arg" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::GetRuntimeArgIndex as usize,
            ),
            "casper_remove_contract_user_group" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::RemoveContractUserGroupIndex as usize,
            ),
            "casper_provision_contract_user_group_uref" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::ExtendContractUserGroupURefsIndex as usize,
            ),
            "casper_remove_contract_user_group_urefs" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 6][..], Some(ValueType::I32)),
                FunctionIndex::RemoveContractUserGroupURefsIndex as usize,
            ),
            "casper_blake2b" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::Blake2b as usize,
            ),
            "casper_load_call_stack" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::LoadCallStack as usize,
            ),
            "casper_load_caller_information" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::LoadCallerInformation as usize,
            ),
            #[cfg(feature = "test-support")]
            "casper_print" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::PrintIndex as usize,
            ),
            "casper_dictionary_get" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::DictionaryGetFuncIndex as usize,
            ),
            "casper_dictionary_read" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 3][..], Some(ValueType::I32)),
                FunctionIndex::DictionaryReadFuncIndex as usize,
            ),
            "casper_dictionary_put" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 6][..], Some(ValueType::I32)),
                FunctionIndex::DictionaryPutFuncIndex as usize,
            ),
            "casper_new_dictionary" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 1][..], Some(ValueType::I32)),
                FunctionIndex::NewDictionaryFuncIndex as usize,
            ),
            "casper_load_authorization_keys" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::LoadAuthorizationKeys as usize,
            ),
            "casper_random_bytes" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], Some(ValueType::I32)),
                FunctionIndex::RandomBytes as usize,
            ),
            "casper_enable_contract_version" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::EnableContractVersion as usize,
            ),
            "casper_manage_message_topic" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::ManageMessageTopic as usize,
            ),
            "casper_emit_message" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 4][..], Some(ValueType::I32)),
                FunctionIndex::EmitMessage as usize,
            ),
            "casper_get_block_info" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 2][..], None),
                FunctionIndex::GetBlockInfoIndex as usize,
            ),
            "casper_generic_hash" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 5][..], Some(ValueType::I32)),
                FunctionIndex::GenericHash as usize,
            ),
            "casper_recover_secp256k1" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 6][..], Some(ValueType::I32)),
                FunctionIndex::RecoverSecp256k1 as usize,
            ),
            "casper_verify_signature" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 6][..], Some(ValueType::I32)),
                FunctionIndex::VerifySignature as usize,
            ),
            "casper_call_package_version" => FuncInstance::alloc_host(
                Signature::new(&[ValueType::I32; 11][..], Some(ValueType::I32)),
                FunctionIndex::CallPackageVersion as usize,
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
