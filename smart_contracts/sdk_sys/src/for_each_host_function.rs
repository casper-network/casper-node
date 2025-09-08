#[macro_export]
macro_rules! for_each_host_function {
    ($mac:ident) => {
        $mac! {
            #[doc = "Read value from a storage available for caller's entity address."]
            pub fn casper_read(
                key_space: u64,
                key_ptr: *const u8,
                key_size: usize,
                info: *mut $crate::ReadInfo,
                alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
                alloc_ctx: *const core::ffi::c_void,
            ) -> u32;
            pub fn casper_write(
                key_space: u64,
                key_ptr: *const u8,
                key_size: usize,
                value_ptr: *const u8,
                value_size: usize,
            ) -> u32;
            pub fn casper_remove(
                key_space: u64,
                key_ptr: *const u8,
                key_size: usize,
            ) -> u32;
            pub fn casper_print(msg_ptr: *const u8, msg_size: usize,);
            pub fn casper_return(flags: u32, data_ptr: *const u8, data_len: usize,);
            pub fn casper_copy_input(
                alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
                alloc_ctx: *const core::ffi::c_void,
            ) -> *mut u8;
            pub fn casper_create(
                code_ptr: *const u8,
                code_size: usize,
                transferred_value: u64,
                constructor_ptr: *const u8,
                constructor_size: usize,
                input_ptr: *const u8,
                input_size: usize,
                seed_ptr: *const u8,
                seed_size: usize,
                result_ptr: *mut $crate::CreateResult,
            ) -> u32;
            pub fn casper_system(
                system_contract_opt: u32,
                input_ptr: *const u8,
                input_size: usize,
                alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8, // For capturing output data
                alloc_ctx: *const core::ffi::c_void,
            ) -> u32;
            // We don't offer any special protection against smart contracts on the host side
            pub fn casper_call(
                address_ptr: *const u8,
                address_size: usize,
                transferred_amount: u64,
                entry_point_ptr: *const u8,
                entry_point_size: usize,
                input_ptr: *const u8,
                input_size: usize,
                alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8, // For capturing output data
                alloc_ctx: *const core::ffi::c_void,
            ) -> u32;
            pub fn casper_upgrade(
                code_ptr: *const u8,
                code_size: usize,
                entry_point_ptr: *const u8,
                entry_point_size: usize,
                input_ptr: *const u8,
                input_size: usize,
            ) -> u32;
            #[doc = r"Get balance of an entity by its address."]
            pub fn casper_env_balance(entity_kind: u32, entity_addr_ptr: *const u8, entity_addr_len: usize, output_ptr: *mut core::ffi::c_void,) -> u32;
            pub fn casper_env_info(info_ptr: *const u8, info_size: u32,) -> u32;
            pub fn casper_transfer(entity_addr_ptr: *const u8, entity_addr_len: usize, amount: *const core::ffi::c_void,) -> u32;
            pub fn casper_emit(topic_ptr: *const u8, topic_size: usize, payload_ptr: *const u8, payload_size: usize,) -> u32;
            pub fn casper_generic_hash(in_ptr: *const u8, in_size: usize, out_ptr: *const u8, hash_algorithm: usize,) -> u32;
            pub fn casper_recover_secp256k1(message_ptr: *const u8, message_size: usize, signature_ptr: *const u8, signature_size: usize, public_key_ptr: *const u8, recovery_id: u32,) -> u32;
        }
    };
}
