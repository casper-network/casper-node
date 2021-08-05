#![no_std]
#![no_main]
#![feature(lang_items)]

use core::arch::wasm32;

const MAX_MEMORY_PAGES: usize = 64;
const GROW_MARGIN: usize = 2;

mod internal_ffi {
    extern "C" {
        pub fn casper_revert(status: u32) -> !;
    }
}

#[repr(u32)]
pub enum ApiError {
    OutOfMemory = 20,
    Unhandled = 31,
}

fn revert(value: ApiError) -> ! {
    unsafe {
        internal_ffi::casper_revert(value as u32);
    }
}

const DEFAULT_MEMORY_INDEX: u32 = 0; // currently wasm spec supports only single memory

pub fn memory_size() -> usize {
    wasm32::memory_size(DEFAULT_MEMORY_INDEX)
}

pub fn memory_grow(new_pages: usize) {
    let ptr = wasm32::memory_grow(DEFAULT_MEMORY_INDEX, new_pages);

    if ptr == usize::max_value() {
        revert(ApiError::OutOfMemory);
    }
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
