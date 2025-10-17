#![no_std]
#![no_main]
#![allow(internal_features)]
#![feature(lang_items)]

extern crate core;

#[cfg(target_arch = "wasm32")]
use core::arch::wasm32;
use core::{ffi::c_void, ptr};

const REVERT_FLAGS: u32 = 0x0000_0001;
const MAX_MEMORY_PAGES: usize = 64;
const GROW_MARGIN: usize = 2;

mod internal_ffi {
    extern "C" {
        pub fn casper_ffi(
            ffi_opt: u32,
            input_ptr: u32,
            input_size: u32,
            alloc: u32,
            alloc_ctx: u32,
        ) -> !;
    }
}

#[repr(u32)]
pub enum ApiError {
    OutOfMemory = 20,
    Unhandled = 31,
}

#[allow(clippy::fn_to_numeric_cast)]
fn revert(value: ApiError) -> ! {
    let mut return_data = [0_u8; 8];
    unsafe {
        extern "C" fn alloc_cb(_len: usize, _ctx: *mut c_void) -> *mut u8 {
            // Return shouldn't have any output data and should not return anything
            ptr::null_mut()
        }
        let rev_flag = REVERT_FLAGS.to_le_bytes();
        for (index, b) in rev_flag.iter().enumerate() {
            return_data[index] = *b;
        }
        let data_flag = (value as u32).to_le_bytes();
        let base_index = rev_flag.len();
        for (index, b) in data_flag.iter().enumerate() {
            return_data[base_index + index] = *b;
        }

        // 600 is code for return
        internal_ffi::casper_ffi(
            600,
            return_data.as_ptr() as u32,
            return_data.len() as u32,
            alloc_cb as u32,
            0_u32,
        );
    }
}

#[cfg(target_arch = "wasm32")]
const DEFAULT_MEMORY_INDEX: u32 = 0; // currently wasm spec supports only single memory

#[cfg(target_arch = "wasm32")]
pub fn memory_size() -> usize {
    wasm32::memory_size(DEFAULT_MEMORY_INDEX)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn memory_size() -> usize {
    revert(ApiError::Unhandled)
}

#[cfg(target_arch = "wasm32")]
pub fn memory_grow(new_pages: usize) {
    let ptr = wasm32::memory_grow(DEFAULT_MEMORY_INDEX, new_pages);

    if ptr == usize::MAX {
        revert(ApiError::OutOfMemory);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn memory_grow(_: usize) {
    revert(ApiError::Unhandled)
}

#[panic_handler]
pub fn panic(_info: &::core::panic::PanicInfo) -> ! {
    revert(ApiError::OutOfMemory)
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[no_mangle]
pub extern "C" fn call() {
    let initial_memory_pages = memory_size();

    // Grow memory into exactly MAX_MEMORY_PAGES - GROW_MARGIN
    memory_grow(MAX_MEMORY_PAGES - initial_memory_pages - GROW_MARGIN);
    assert_eq!(memory_size(), MAX_MEMORY_PAGES - GROW_MARGIN);

    // Now we are occupying exactly MAX_MEMORY_PAGES
    memory_grow(GROW_MARGIN);
    assert_eq!(memory_size(), MAX_MEMORY_PAGES);

    // This will fail
    memory_grow(1);
}
