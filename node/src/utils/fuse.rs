/// Fuses of various kind.
///
/// A fuse is a boolean flag that can only be set once, but checked any number of times.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use datasize::DataSize;
use tokio::sync::Notify;

use super::leak;

/// A one-time settable boolean flag.
pub(crate) trait Fuse {
    /// Trigger the fuse.
    fn set(&self);
}

/// A set-once-only flag shared across multiple subsystems.
#[derive(Copy, Clone, DataSize, Debug)]
pub(crate) struct SharedFuse(&'static AtomicBool);

impl SharedFuse {
    /// Creates a new shared fuse.
    ///
    /// The fuse is initially not set.
    pub(crate) fn new() -> Self {
        SharedFuse(leak(AtomicBool::new(false)))
    }

    /// Checks whether the fuse is set.
    pub(crate) fn is_set(self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Returns a shared instance of the fuse for testing.
    ///
    /// The returned fuse should **never** have `set` be called upon it, since there is only once
    /// instance globally.
    #[cfg(test)]
    pub(crate) fn global_shared() -> Self {
        use once_cell::sync::Lazy;

        static SHARED_FUSE: Lazy<SharedFuse> = Lazy::new(SharedFuse::new);

        *SHARED_FUSE
    }
}

impl Fuse for SharedFuse {
    fn set(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
}

impl Default for SharedFuse {
    fn default() -> Self {
        Self::new()
    }
}

/// A shared fuse that can be observed for change.
///
/// It is similar to a condition var, except it can only bet set once and will immediately return
/// if it was previously set.
#[derive(DataSize, Clone, Debug)]
pub(crate) struct ObservableFuse(Arc<ObservableFuseInner>);

impl ObservableFuse {
    /// Creates a new sticky fuse.
    ///
    /// The fuse will start out as not set.
    pub(crate) fn new() -> Self {
        ObservableFuse(Arc::new(ObservableFuseInner {
            fuse: AtomicBool::new(false),
            notify: Notify::new(),
        }))
    }
}

/// Inner implementation of the `ObservableFuse`.
#[derive(DataSize, Debug)]
struct ObservableFuseInner {
    /// The fuse to trigger.
    #[data_size(skip)]
    fuse: AtomicBool,
    /// Notification that the fuse has been triggered.
    #[data_size(skip)]
    notify: Notify,
}

impl ObservableFuse {
    /// Waits for the fuse to be triggered.
    ///
    /// If the fuse is already set, returns immediately, otherwise waits for the notification.
    ///
    /// The future returned by this function is safe to cancel.
    pub(crate) async fn wait(&self) {
        // Note: We will catch all notifications from the point on where `notified()` is called, so
        //       we first construct the future, then check the fuse. Any notification sent while we
        //       were loading will be caught in the `notified.await`.
        let notified = self.0.notify.notified();

        if self.0.fuse.load(Ordering::SeqCst) {
            return;
        }

        notified.await;
    }
}

impl Fuse for ObservableFuse {
    fn set(&self) {
        self.0.fuse.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
}

/// A wrapper for a fuse that will cause it to be set when dropped.
#[derive(DataSize, Debug, Clone)]
pub(crate) struct DropSwitch<T>(T)
where
    T: Fuse;

impl<T> DropSwitch<T>
where
    T: Fuse,
{
    /// Creates a new drop switch around a fuse.
    pub(crate) fn new(fuse: T) -> Self {
        DropSwitch(fuse)
    }

    /// Access the wrapped fuse.
    pub(crate) fn inner(&self) -> &T {
        &self.0
    }
}

impl<T> Drop for DropSwitch<T>
where
    T: Fuse,
{
    fn drop(&mut self) {
        self.0.set()
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use crate::utils::Fuse;

    use super::{DropSwitch, ObservableFuse, SharedFuse};

    #[test]
    fn shared_fuse_sanity_check() {
        let fuse = SharedFuse::new();
        let copied = fuse;

        assert!(!fuse.is_set());
        assert!(!copied.is_set());
        assert!(!fuse.is_set());
        assert!(!copied.is_set());

        fuse.set();

        assert!(fuse.is_set());
        assert!(copied.is_set());
        assert!(fuse.is_set());
        assert!(copied.is_set());
    }

    #[test]
    fn observable_fuse_sanity_check() {
        let fuse = ObservableFuse::new();
        assert!(fuse.wait().now_or_never().is_none());

        fuse.set();

        // Should finish immediately due to the fuse being set.
        assert!(fuse.wait().now_or_never().is_some());
    }

    #[test]
    fn observable_fuse_drop_switch_check() {
        let fuse = ObservableFuse::new();
        assert!(fuse.wait().now_or_never().is_none());

        let drop_switch = DropSwitch::new(fuse.clone());
        assert!(fuse.wait().now_or_never().is_none());

        drop(drop_switch);
        assert!(fuse.wait().now_or_never().is_some());
    }

    #[test]
    fn observable_fuse_race_condition_check() {
        let fuse = ObservableFuse::new();
        assert!(fuse.wait().now_or_never().is_none());

        let waiting = fuse.wait();
        fuse.set();
        assert!(waiting.now_or_never().is_some());
    }
}
