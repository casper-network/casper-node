//! Contains definitions for panic and allocation error handlers.

/// A panic handler for use in a `no_std` environment which simply aborts the process.
#[panic_handler]
pub fn panic(_info: &core::panic::PanicInfo) -> ! {
    #[cfg(feature = "test-support")]
    crate::contract_api::runtime::print(&alloc::format!("{_info}"));
    abort()
}

#[cfg(target_arch = "wasm32")]
fn abort() -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(not(target_arch = "wasm32"))]
fn abort() -> ! {
    loop {
        core::hint::spin_loop()
    }
}
