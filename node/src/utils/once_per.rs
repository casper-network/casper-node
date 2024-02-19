//! Rate limiting for log messages.
//!
//! Implements the `rate_limited!` macro which can be used to ensure that a log message does not
//! spam the logs if triggered many times in a row. See its documentation for details.

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Maximum interval for spammable warnings.
pub(crate) const DEFAULT_WARNING_INTERVAL: Duration = Duration::from_secs(60);

/// Macro for rate limiting a log message.
///
/// Every rate limiter needs a unique identifier, which is used to create a static variable holding
/// the count and time of last update.
///
/// **Rate limiting is not free**. Every call of this macro, even if the log message ultimately not
/// emitted due to log settings, requires a `Mutex` lock to be acquired!
///
/// ## Example usage
///
/// The `rate_limited!` macro expects at least two arguments, the identifier described above, and a
/// function taking a single `usize` argument that will be called to make the actual log message.
/// The argument is the number of times this call has been skipped since the last time it was
/// called.
///
/// ```
/// rate_limited!(CONNECTION_THRESHOLD_EXCEEDED, |count| warn!(count, "exceeded connection threshold"));
/// ```
macro_rules! rate_limited {
    ($key:ident, $action:expr) => {
        rate_limited!(
            $key,
            $crate::utils::once_per::DEFAULT_WARNING_INTERVAL,
            $action
        );
    };
    ($key:ident, $ival:expr, $action:expr) => {
        static $key: $crate::utils::once_per::OncePer = $crate::utils::once_per::OncePer::new();

        if let Some(skipped) = $key.active($ival) {
            $action(skipped);
        }
    };
}
pub(crate) use rate_limited;

/// Helper struct for the `rate_limited!` macro.
///
/// There is usually little use in constructing these directly.
#[derive(Debug)]
pub(crate) struct OncePer(OnceLock<Mutex<OncePerData>>);

/// Data tracking calling of [`OncePer`] via `rate_limited!`.
#[derive(Default, Debug)]
pub(crate) struct OncePerData {
    /// Last time [`OncePerData::active`] was called, or `None` if never.
    last: Option<Instant>,
    /// Number of times the callback function was not executed since the last execution.
    skipped: usize,
}

impl OncePer {
    /// Constructs a new instance.
    pub(crate) const fn new() -> Self {
        Self(OnceLock::new())
    }

    /// Checks if the last call is sufficiently in the past to trigger.
    ///
    /// Returns the number of times `active` has been called as `Some` if the trigger condition has
    /// been met, otherwise `None`.
    pub(crate) fn active(&self, max_interval: Duration) -> Option<usize> {
        let mut guard = self
            .0
            .get_or_init(|| Mutex::new(OncePerData::default()))
            .lock()
            .expect("lock poisoned");

        let now = Instant::now();
        if let Some(last) = guard.last {
            if now.duration_since(last) < max_interval {
                // We already fired.
                guard.skipped += 1;

                return None;
            }
        }

        guard.last = Some(now);
        let skipped = guard.skipped;
        guard.skipped = 0;
        Some(skipped)
    }
}
