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
            ) -> i32;
            pub fn casper_write(
                key_space: u64,
                key_ptr: *const u8,
                key_size: usize,
                value_ptr: *const u8,
                value_size: usize,
            ) -> i32;
            pub fn casper_print(msg_ptr: *const u8, msg_size: usize,);
            pub fn casper_return(flags: u32, data_ptr: *const u8, data_len: usize,);
            pub fn casper_copy_input(
                alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
                alloc_ctx: *const core::ffi::c_void,
            ) -> *mut u8;
            pub fn casper_create(
                code_ptr: *const u8,
                code_size: usize,
                value: u64,
                constructor_ptr: *const u8,
                constructor_size: usize,
                input_ptr: *const u8,
                input_size: usize,
                result_ptr: *mut $crate::CreateResult,
            ) -> u32;
            pub fn casper_call(
                address_ptr: *const u8,
                address_size: usize,
                value: u64,
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

            #[doc = r"Obtain data from the blockchain environemnt of current wasm invocation.

Example paths:

* `env_read([CASPER_CALLER], 1, nullptr, &caller_addr)` -> read caller's address into
  `caller_addr` memory.
* `env_read([CASPER_CHAIN, BLOCK_HASH, 0], 3, nullptr, &block_hash)` -> read hash of the
  current block into `block_hash` memory.
* `env_read([CASPER_CHAIN, BLOCK_HASH, 5], 3, nullptr, &block_hash)` -> read hash of the 5th
  block from the current one into `block_hash` memory.
* `env_read([CASPER_AUTHORIZED_KEYS], 1, nullptr, &authorized_keys)` -> read list of
  authorized keys into `authorized_keys` memory."]
            pub fn casper_env_read(
                env_path: *const u64,
                env_path_size: usize,
                alloc: Option<extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8>, // For capturing output data
                alloc_ctx: *const core::ffi::c_void,
            ) -> *mut u8;
            pub fn casper_env_caller(dest: *mut u8, dest_len: usize, entity_kind: *mut u32,) -> *const u8;
            pub fn casper_env_value() -> u64;
            #[doc = r"Get balance of an entity by its address."]
            pub fn casper_env_balance(entity_kind: u32, entity_addr_ptr: *const u8, entity_addr_len: usize,) -> u64;
            pub fn casper_transfer(entity_kind: u32, entity_addr_ptr: *const u8, entity_addr_len: usize, amount: u64,) -> u32;
        }
    };
}
