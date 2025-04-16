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
            pub fn casper_print(msg_ptr: *const u8, msg_size: usize,);
            pub fn casper_return(flags: u32, data_ptr: *const u8, data_len: usize,);
            pub fn casper_copy_input(
                alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
                alloc_ctx: *const core::ffi::c_void,
            ) -> *mut u8;
            pub fn casper_create(
                code_ptr: *const u8,
                code_size: usize,
                value: *const core::ffi::c_void,
                constructor_ptr: *const u8,
                constructor_size: usize,
                input_ptr: *const u8,
                input_size: usize,
                seed_ptr: *const u8,
                seed_size: usize,
                result_ptr: *mut $crate::CreateResult,
            ) -> u32;

            // We don't offer any special protection against smart contracts on the host side
            pub fn casper_call(
                address_ptr: *const u8,
                address_size: usize,
                transferred_amount: *const core::ffi::c_void,
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
            pub fn casper_env_caller(dest: *mut u8, dest_len: usize, entity_kind: *mut u32,) -> *const u8;
            pub fn casper_env_transferred_value(dest: *mut core::ffi::c_void,);
            #[doc = r"Get balance of an entity by its address."]
            pub fn casper_env_balance(entity_kind: u32, entity_addr_ptr: *const u8, entity_addr_len: usize, output_ptr: *mut core::ffi::c_void,) -> u32;
            pub fn casper_env_block_time() -> u64;

            pub fn casper_transfer(entity_addr_ptr: *const u8, entity_addr_len: usize, amount: *const core::ffi::c_void,) -> u32;
            pub fn casper_emit(topic_ptr: *const u8, topic_size: usize, payload_ptr: *const u8, payload_size: usize,) -> u32;
        }
    };
}
