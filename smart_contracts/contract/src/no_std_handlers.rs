//! Contains definitions for panic and allocation error handlers.

/// A panic handler for use in a `no_std` environment which simply aborts the process.
#[panic_handler]
#[no_mangle]
pub fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::intrinsics::abort();
}

/// An out-of-memory allocation error handler for use in a `no_std` environment which simply aborts
/// the process.
#[alloc_error_handler]
#[no_mangle]
pub fn oom(_: core::alloc::Layout) -> ! {
    core::intrinsics::abort();
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}
