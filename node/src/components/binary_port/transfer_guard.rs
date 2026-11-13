//! Protection against peers that hold one of the limited connection slots by transferring data
//! too slowly, in either direction:
//!
//! * **receiving** - a peer starts sending a message but never finishes it (e.g. declares a frame
//!   length and then stalls, or trickles the body),
//! * **sending** - a peer sends a request but doesn't read the response, so writing it to the
//!   socket blocks.
//!
//! Both directions are guarded by the same [`ThroughputGuard`], which measures a single transfer.
//! A guard only exists while a transfer is in progress. Between messages no guard is active and a
//! connection is never dropped for being silent - the lifetime of an otherwise silent connection is
//! governed solely by the `ConnectionTerminator` (`initial_connection_lifetime` and the
//! `*_termination_delay` extensions).
//!
//! Each transfer starts with a grace period during which the connection is never dropped.
//! Afterwards a **minimum throughput** is enforced: time is divided into consecutive windows of the
//! grace period's length, starting with the transfer (so data moved during the grace period counts
//! towards the first window). At the end of each window, the average throughput within it must not
//! fall below the configured minimum. Evaluating every window separately (rather than averaging
//! since the start) prevents a peer from banking time with an initial burst and then trickling the
//! remainder - a burst covers at most the window it falls in.

use std::{
    fmt::{self, Display, Formatter},
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::Instant,
};

/// Upper bound on how often the guards are evaluated.
const MAX_CHECK_INTERVAL: Duration = Duration::from_millis(250);
/// Lower bound on how often the guards are evaluated, so a tiny grace period doesn't turn the
/// checks into a busy loop.
const MIN_CHECK_INTERVAL: Duration = Duration::from_millis(10);

/// How often a guard with the given grace period is evaluated: twice per grace period (bounded),
/// so the grace period actually ends close to its configured length rather than on a coarse tick.
pub(super) fn check_interval(grace_period: Duration) -> Duration {
    (grace_period / 2).clamp(MIN_CHECK_INTERVAL, MAX_CHECK_INTERVAL)
}

/// Direction of a guarded transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    /// Receiving a message from the peer.
    Receive,
    /// Sending a response to the peer.
    Send,
}

impl Display for Direction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Direction::Receive => write!(formatter, "receiving a message"),
            Direction::Send => write!(formatter, "sending a response"),
        }
    }
}

/// Reason for which a connection is dropped: only `bytes` were transferred within `window`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferViolation {
    pub(crate) direction: Direction,
    pub(crate) bytes: u64,
    pub(crate) window: Duration,
    pub(crate) min_bytes_per_second: u32,
}

impl Display for TransferViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "transferred {} bytes in {}ms {}, minimum is {} B/s",
            self.bytes,
            self.window.as_millis(),
            self.direction,
            self.min_bytes_per_second
        )
    }
}

/// Limits applied to a single transfer.
#[derive(Debug, Clone, Copy)]
pub(super) struct TransferLimits {
    /// Minimum throughput after the grace period; `0` disables the check.
    pub(super) min_bytes_per_second: u32,
    /// Time from the start of the transfer during which the connection is never dropped. Also
    /// the length of the window over which throughput is measured.
    pub(super) grace_period: Duration,
}

/// Guards a single transfer (receiving one message or sending one response).
pub(super) struct ThroughputGuard {
    direction: Direction,
    limits: TransferLimits,
    started_at: Instant,
    /// `(started_at, total_bytes)` at the start of the current measurement window.
    window_start: (Instant, u64),
}

impl ThroughputGuard {
    /// Starts guarding a transfer. `base_bytes` is the value of the byte counter attributed to
    /// the start of the transfer.
    pub(super) fn new(
        direction: Direction,
        limits: TransferLimits,
        now: Instant,
        base_bytes: u64,
    ) -> Self {
        ThroughputGuard {
            direction,
            limits,
            started_at: now,
            window_start: (now, base_bytes),
        }
    }

    fn window(&self) -> Duration {
        self.limits
            .grace_period
            .max(check_interval(self.limits.grace_period))
    }

    /// Evaluates the transfer given the current value of the byte counter.
    pub(super) fn check(
        &mut self,
        now: Instant,
        total_bytes: u64,
    ) -> Result<(), TransferViolation> {
        if self.limits.min_bytes_per_second == 0 {
            return Ok(());
        }
        if now.saturating_duration_since(self.started_at) < self.limits.grace_period {
            // Collect, but never drop during the grace period.
            return Ok(());
        }
        // Throughput is evaluated once per completed window; the first window starts with the
        // transfer, so it covers the grace period.
        let (window_started_at, window_start_bytes) = self.window_start;
        let window = now.saturating_duration_since(window_started_at);
        if window < self.window() {
            return Ok(());
        }
        let bytes = total_bytes.saturating_sub(window_start_bytes);
        let required = u128::from(self.limits.min_bytes_per_second) * window.as_millis();
        if u128::from(bytes) * 1000 < required {
            return Err(TransferViolation {
                direction: self.direction,
                bytes,
                window,
                min_bytes_per_second: self.limits.min_bytes_per_second,
            });
        }
        self.window_start = (now, total_bytes);
        Ok(())
    }
}

/// Stream wrapper counting received and sent bytes.
pub(super) struct CountingStream<S> {
    inner: S,
    total_bytes_read: u64,
    /// Shared, so it can be sampled while the stream is mutably borrowed by a pending send.
    total_bytes_written: Arc<AtomicU64>,
}

impl<S> CountingStream<S> {
    pub(super) fn new(inner: S) -> Self {
        CountingStream {
            inner,
            total_bytes_read: 0,
            total_bytes_written: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(super) fn total_bytes_read(&self) -> u64 {
        self.total_bytes_read
    }

    pub(super) fn bytes_written_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.total_bytes_written)
    }

    fn count_written(&self, result: &Poll<io::Result<usize>>) {
        if let Poll::Ready(Ok(written)) = result {
            self.total_bytes_written
                .fetch_add(*written as u64, Ordering::Relaxed);
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for CountingStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let filled_before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = result {
            let read = buf.filled().len().saturating_sub(filled_before);
            this.total_bytes_read = this.total_bytes_read.saturating_add(read as u64);
        }
        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for CountingStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, buf);
        this.count_written(&result);
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write_vectored(cx, bufs);
        this.count_written(&result);
        result
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRACE: Duration = Duration::from_secs(2);
    const MIN_RATE: u32 = 1000;
    /// `check_interval(GRACE)`.
    const STEP: Duration = MAX_CHECK_INTERVAL;

    const LIMITS: TransferLimits = TransferLimits {
        min_bytes_per_second: MIN_RATE,
        grace_period: GRACE,
    };

    /// Every behaviour must be identical in both directions.
    const DIRECTIONS: [Direction; 2] = [Direction::Receive, Direction::Send];

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn check_interval_should_follow_grace_period() {
        assert_eq!(check_interval(GRACE), STEP);
        assert_eq!(check_interval(ms(100)), ms(50));
        assert_eq!(check_interval(Duration::ZERO), MIN_CHECK_INTERVAL);
    }

    #[test]
    fn should_report_direction() {
        for direction in DIRECTIONS {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, LIMITS, start, 0);
            let violation = guard.check(start + GRACE, 0).unwrap_err();
            assert_eq!(violation.direction, direction);
        }
    }

    #[test]
    fn should_not_drop_during_grace_period() {
        for direction in DIRECTIONS {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, LIMITS, start, 0);
            let mut now = start;
            while now + STEP < start + GRACE {
                now += STEP;
                assert!(guard.check(now, 0).is_ok());
            }
            assert!(guard.check(start + GRACE, 0).is_err());
        }
    }

    #[test]
    fn should_drop_transfer_without_progress_right_after_grace_period() {
        for direction in DIRECTIONS {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, LIMITS, start, 4);
            let mut now = start;
            let violation = loop {
                now += STEP;
                if let Err(violation) = guard.check(now, 4) {
                    break violation;
                }
            };
            // Dropped at the end of the first window, which ends with the grace period.
            assert_eq!(violation.bytes, 0);
            assert_eq!(now - start, GRACE);
        }
    }

    #[test]
    fn should_drop_trickling_transfer() {
        for direction in DIRECTIONS {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, LIMITS, start, 0);
            let (mut now, mut total) = (start, 0);
            // One byte per check: always some progress, but far below the minimum rate.
            let result = loop {
                now += STEP;
                total += 1;
                let result = guard.check(now, total);
                if result.is_err() {
                    break result;
                }
            };
            assert!(result.is_err());
            assert_eq!(now - start, GRACE);
        }
    }

    #[test]
    fn should_drop_burst_then_trickle() {
        for direction in DIRECTIONS {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, LIMITS, start, 0);
            // A large burst (or kernel buffers absorbing one)...
            let (mut now, mut total) = (start, 1_000_000);
            assert!(guard.check(now, total).is_ok());
            // ...followed by a trickle. The burst only covers the window it falls in.
            let result = loop {
                now += STEP;
                total += 1;
                let result = guard.check(now, total);
                if result.is_err() {
                    break result;
                }
            };
            assert!(result.is_err());
            assert_eq!(now - start, GRACE * 2);
        }
    }

    #[test]
    fn should_count_data_collected_during_grace_period() {
        for direction in DIRECTIONS {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, LIMITS, start, 0);
            // Enough for the whole first window arrives during the grace period, then nothing.
            let enough = u64::from(MIN_RATE) * GRACE.as_secs();
            assert!(guard.check(start + ms(100), enough).is_ok());
            // The first window, ending with the grace period, is satisfied by that data.
            assert!(guard.check(start + GRACE, enough).is_ok());
            // The next window is evaluated on its own, once it completes.
            assert!(guard.check(start + GRACE * 2 - ms(1), enough).is_ok());
            assert!(guard.check(start + GRACE * 2, enough).is_err());
        }
    }

    #[test]
    fn should_keep_transfer_meeting_minimum_rate() {
        for grace_period in [GRACE, ms(100)] {
            for direction in DIRECTIONS {
                let limits = TransferLimits {
                    grace_period,
                    ..LIMITS
                };
                let step = check_interval(grace_period);
                let per_step = u64::from(MIN_RATE) * step.as_millis() as u64 / 1000;
                let start = Instant::now();
                let mut guard = ThroughputGuard::new(direction, limits, start, 0);
                let (mut now, mut total) = (start, 0);
                for _ in 0..100 {
                    now += step;
                    total += per_step;
                    assert!(guard.check(now, total).is_ok());
                }
            }
        }
    }

    #[test]
    fn should_enforce_short_grace_period_at_its_configured_length() {
        for direction in DIRECTIONS {
            let limits = TransferLimits {
                grace_period: ms(100),
                ..LIMITS
            };
            let step = check_interval(limits.grace_period);
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, limits, start, 0);
            assert!(guard.check(start + step, 0).is_ok());
            assert!(guard.check(start + step * 2, 0).is_err());
        }
    }

    #[test]
    fn should_not_enforce_rate_when_disabled() {
        for direction in DIRECTIONS {
            let limits = TransferLimits {
                min_bytes_per_second: 0,
                ..LIMITS
            };
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(direction, limits, start, 0);
            let mut now = start;
            for _ in 0..100 {
                now += STEP;
                assert!(guard.check(now, 0).is_ok());
            }
        }
    }
}
