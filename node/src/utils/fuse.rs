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

/// A set-once-only flag shared across multiple subsystems.
#[derive(Copy, Clone, DataSize, Debug)]
pub(crate) struct SharedFuse(&'static AtomicBool);

impl SharedFuse {
    /// Creates a new shared flag.
    ///
    /// The flag is initially not set.
    pub(crate) fn new() -> Self {
        SharedFuse(leak(AtomicBool::new(false)))
    }

    /// Checks whether the flag is set.
    pub(crate) fn is_set(self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Set the flag.
    pub(crate) fn set(self) {
        self.0.store(true, Ordering::SeqCst)
    }

    /// Returns a shared instance of the flag for testing.
    ///
    /// The returned flag should **never** have `set` be called upon it, since there is only once
    /// instance globally.
    #[cfg(test)]
    pub(crate) fn global_shared() -> Self {
        use once_cell::sync::Lazy;

        static SHARED_FLAG: Lazy<SharedFuse> = Lazy::new(SharedFuse::new);

        *SHARED_FLAG
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
#[derive(Clone, Debug)]
pub(crate) struct ObservableFuse(Arc<ObservableFuseInner>);

impl ObservableFuse {
    /// Creates a new sticky flag.
    ///
    /// The flag will start out as not set.
    pub(crate) fn new() -> Self {
        ObservableFuse(Arc::new(ObservableFuseInner {
            flag: AtomicBool::new(false),
            notify: Notify::new(),
        }))
    }

    /// Creates a new sticky flag drop switch.
    pub(crate) fn drop_switch(&self) -> ObservableFuseDropSwitch {
        ObservableFuseDropSwitch(self.clone())
    }
}

/// Inner implementation of the `StickyFlag`.
#[derive(Debug)]
struct ObservableFuseInner {
    /// The flag to be cleared.
    flag: AtomicBool,
    /// Notification that the flag has been changed.
    notify: Notify,
}

impl ObservableFuse {
    /// Sets the flag.
    ///
    /// Will always send a notification, regardless of whether the flag was actually changed.
    pub(crate) fn set(&self) {
        self.0.flag.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }

    /// Waits for the flag to be set.
    ///
    /// If the flag is already set, returns immediately, otherwise waits for the notification.
    ///
    /// The future returned by this function is safe to cancel.
    pub(crate) async fn wait(&self) {
        // Note: We will catch all notifications from the point on where `notified()` is called, so
        //       we first construct the future, then check the flag. Any notification sent while we
        //       were loading will be caught in the `notified.await`.
        let notified = self.0.notify.notified();

        if self.0.flag.load(Ordering::SeqCst) {
            return;
        }

        notified.await;
    }
}

/// A wrapper for an observable fuse that will cause it to be set when dropped.
#[derive(Debug, Clone)]
pub(crate) struct ObservableFuseDropSwitch(ObservableFuse);

impl Drop for ObservableFuseDropSwitch {
    fn drop(&mut self) {
        self.0.set()
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::{ObservableFuse, ObservableFuseDropSwitch, SharedFuse};

    #[test]
    fn shared_fuse_sanity_check() {
        let flag = SharedFuse::new();
        let copied = flag;

        assert!(!flag.is_set());
        assert!(!copied.is_set());
        assert!(!flag.is_set());
        assert!(!copied.is_set());

        flag.set();

        assert!(flag.is_set());
        assert!(copied.is_set());
        assert!(flag.is_set());
        assert!(copied.is_set());
    }

    #[test]
    fn observable_fuse_sanity_check() {
        let flag = ObservableFuse::new();
        assert!(flag.wait().now_or_never().is_none());

        flag.set();

        // Should finish immediately due to the flag being set.
        assert!(flag.wait().now_or_never().is_some());
    }

    #[test]
    fn observable_fuse_drop_switch_check() {
        let flag = ObservableFuse::new();
        assert!(flag.wait().now_or_never().is_none());

        let drop_switch = flag.drop_switch();
        assert!(flag.wait().now_or_never().is_none());

        drop(drop_switch);
        assert!(flag.wait().now_or_never().is_some());
    }

    #[test]
    fn sticky_flag_race_condition_check() {
        let flag = ObservableFuse::new();
        assert!(flag.wait().now_or_never().is_none());

        let waiting = flag.wait();
        flag.set();
        assert!(waiting.now_or_never().is_some());
    }
}
