//! Umask setting functions.

/// File mode.
pub(crate) type Mode = libc::mode_t;

/// Set the umask to `new_mode`, returning the current mode.
fn umask(new_mode: Mode) -> Mode {
    // `umask` is always successful (according to the manpage), so there is no error condition to
    // check.
    unsafe { libc::umask(new_mode) }
}

/// Sets a new umask, returning a guard that will restore the current umask when dropped.
pub(crate) fn temp_umask(new_mode: Mode) -> UmaskGuard {
    let prev = umask(new_mode);
    UmaskGuard { prev }
}

/// Guard for umask, will restore the contained umask on drop.
#[derive(Clone, Debug)]
pub(crate) struct UmaskGuard {
    /// The mode stored in the guard.
    prev: Mode,
}

impl Drop for UmaskGuard {
    fn drop(&mut self) {
        umask(self.prev);
    }
}
