//! Protection against peers that hold one of the limited connection slots by transferring data
//! too slowly, in either direction:
//!
//! * **receiving** - a peer starts sending a message but never finishes it (e.g. declares a frame
//!   length and then stalls, or trickles the body),
//! * **sending** - a peer sends a request but doesn't read the response, so writing it to the
//!   socket blocks.
//!
//! Both directions are guarded by a [`ThroughputGuard`], which measures a single transfer.
//! A guard only exists while a transfer is in progress. Between messages no guard is active and a
//! connection is never dropped for being silent - the lifetime of an otherwise silent connection is
//! governed solely by the `ConnectionTerminator` (`initial_connection_lifetime` and the
//! `*_termination_delay` extensions). A response in flight is governed solely by its guard: a
//! connection whose lifetime runs out while a response is being sent is closed only once the
//! response has been sent. If the minimum throughput is `0` there is no guard to bound the send,
//! so the lifetime then also interrupts responses in flight.
//!
//! Each transfer starts with a grace period during which the connection is never dropped.
//! Afterwards a **minimum throughput** is enforced, differently in each direction:
//!
//! * **receiving** - time is divided into consecutive windows of the grace period's length, starting
//!   with the transfer (so data moved during the grace period counts towards the first window). At
//!   the end of each window, the average throughput within it must not fall below the configured
//!   minimum. A burst covers at most the window it falls in.
//! * **sending** - the number of bytes written only tells how much the kernel accepted, not how much
//!   the peer read. The first writes fill the socket buffers at once, and afterwards the socket
//!   becomes writable again only when a large part of them has drained. A peer reading steadily thus
//!   shows up as rare, large jumps, with many windows without any progress. Sending is therefore
//!   guarded by a deadline, which initially is the end of the grace period and which every byte
//!   written moves by the time it takes to transfer it at the minimum throughput. To prevent a peer
//!   from banking time without limit by reading quickly and then stalling, the deadline is never
//!   further away than [`MAX_SEND_CREDIT`] (or the grace period, if that's longer).

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

pub(super) fn clamp_interval(grace_period: Duration) -> Duration {
    (grace_period / 2).clamp(MIN_CHECK_INTERVAL, MAX_CHECK_INTERVAL)
}

/// Upper bound on how far ahead the deadline of a send can be, i.e. on how long a peer which stopped
/// reading can hold its connection. Must be longer than the time a peer reading at the minimum
/// throughput needs to drain enough of the socket buffers for the socket to be writable again.
const MAX_SEND_CREDIT: Duration = Duration::from_secs(30);

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
    /// the length of the window over which throughput is measured when receiving.
    pub(super) grace_period: Duration,
}

/// Guards a single transfer (receiving one message or sending one response).
pub(super) struct ThroughputGuard {
    direction: Direction,
    limits: TransferLimits,
    started_at: Instant,
    /// `(started_at, total_bytes)` at the start of the current measurement window. When sending,
    /// the window starts when the credit was last capped (or with the transfer).
    window_start: (Instant, u64),
    /// When sending: the deadline for further progress.
    send_deadline: Instant,
    /// When sending: the value of the byte counter already credited to `send_deadline`.
    credited_bytes: u64,
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
        let mut guard = ThroughputGuard {
            direction,
            limits,
            started_at: now,
            window_start: (now, base_bytes),
            send_deadline: now,
            credited_bytes: base_bytes,
        };
        guard.send_deadline += guard.window();
        guard
    }

    fn window(&self) -> Duration {
        self.limits
            .grace_period
            .max(clamp_interval(self.limits.grace_period))
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
        match self.direction {
            Direction::Receive => self.check_receive(now, total_bytes),
            Direction::Send => self.check_send(now, total_bytes),
        }
    }

    fn check_receive(&mut self, now: Instant, total_bytes: u64) -> Result<(), TransferViolation> {
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
            return Err(self.violation(now, total_bytes));
        }
        self.window_start = (now, total_bytes);
        Ok(())
    }

    fn check_send(&mut self, now: Instant, total_bytes: u64) -> Result<(), TransferViolation> {
        let new_bytes = total_bytes.saturating_sub(self.credited_bytes);
        self.credited_bytes = total_bytes;
        // `min_bytes_per_second` is not `0` here, the check is disabled then.
        let earned_micros =
            u128::from(new_bytes) * 1_000_000 / u128::from(self.limits.min_bytes_per_second);
        let earned = Duration::from_micros(u64::try_from(earned_micros).unwrap_or(u64::MAX));
        // The cap is never shorter than the grace period, so it can't cut the grace period short.
        let max_deadline = now + MAX_SEND_CREDIT.max(self.window());
        match self.send_deadline.checked_add(earned) {
            Some(deadline) if deadline < max_deadline => self.send_deadline = deadline,
            _ => {
                self.send_deadline = max_deadline;
                self.window_start = (now, total_bytes);
            }
        }
        if now >= self.send_deadline {
            return Err(self.violation(now, total_bytes));
        }
        Ok(())
    }

    fn violation(&self, now: Instant, total_bytes: u64) -> TransferViolation {
        let (window_started_at, window_start_bytes) = self.window_start;
        TransferViolation {
            direction: self.direction,
            bytes: total_bytes.saturating_sub(window_start_bytes),
            window: now.saturating_duration_since(window_started_at),
            min_bytes_per_second: self.limits.min_bytes_per_second,
        }
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

    /// For behaviours which must be identical in both directions.
    const DIRECTIONS: [Direction; 2] = [Direction::Receive, Direction::Send];

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn check_interval_should_follow_grace_period() {
        assert_eq!(clamp_interval(GRACE), STEP);
        assert_eq!(clamp_interval(ms(100)), ms(50));
        assert_eq!(clamp_interval(Duration::ZERO), MIN_CHECK_INTERVAL);
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
            // When sending, the few bytes sent extend the deadline slightly past the grace period,
            // so it's the next check that fails.
            let expected = match direction {
                Direction::Receive => GRACE,
                Direction::Send => GRACE + STEP,
            };
            assert_eq!(now - start, expected);
        }
    }

    /// Checks every `STEP` after `start`, with `trickle` more bytes each time, until the transfer
    /// is dropped.
    fn run_until_dropped(
        guard: &mut ThroughputGuard,
        start: Instant,
        mut total: u64,
        trickle: u64,
    ) -> (Duration, TransferViolation) {
        let mut now = start;
        loop {
            now += STEP;
            total += trickle;
            if let Err(violation) = guard.check(now, total) {
                return (now - start, violation);
            }
            assert!(now - start < MAX_SEND_CREDIT * 10, "not dropped");
        }
    }

    #[test]
    fn should_drop_receive_burst_then_trickle() {
        let start = Instant::now();
        let mut guard = ThroughputGuard::new(Direction::Receive, LIMITS, start, 0);
        // A large burst followed by a trickle. The burst only covers the window it falls in.
        assert!(guard.check(start, 1_000_000).is_ok());
        let (elapsed, _) = run_until_dropped(&mut guard, start, 1_000_000, 1);
        assert_eq!(elapsed, GRACE * 2);
    }

    #[test]
    fn should_drop_stalled_send_after_max_credit_regardless_of_burst_size() {
        for burst in [1_000_000, 1_000_000_000, u64::MAX] {
            let start = Instant::now();
            let mut guard = ThroughputGuard::new(Direction::Send, LIMITS, start, 0);
            // The burst is credited by the first check after the grace period.
            assert!(guard.check(start + GRACE, burst).is_ok());
            let (elapsed, violation) = run_until_dropped(&mut guard, start + GRACE, burst, 0);
            assert_eq!(elapsed, MAX_SEND_CREDIT);
            // The violation describes the stall, not the burst before it.
            assert_eq!(violation.bytes, 0);
            assert_eq!(violation.window, MAX_SEND_CREDIT);
        }
    }

    #[test]
    fn should_drop_send_burst_then_trickle() {
        let start = Instant::now();
        let mut guard = ThroughputGuard::new(Direction::Send, LIMITS, start, 0);
        assert!(guard.check(start + GRACE, 1_000_000).is_ok());
        let (elapsed, violation) = run_until_dropped(&mut guard, start + GRACE, 1_000_000, 1);
        // The trickle (1/250 of the minimum rate) buys next to no time.
        assert!(elapsed >= MAX_SEND_CREDIT, "{elapsed:?}");
        assert!(elapsed < MAX_SEND_CREDIT + GRACE, "{elapsed:?}");
        // The violation reports a throughput below the minimum.
        assert!(violation.bytes < u64::from(MIN_RATE) * violation.window.as_secs());
    }

    #[test]
    fn should_keep_send_progressing_in_rare_jumps() {
        // Progress as seen through socket buffers drained at the minimum rate: an initial burst,
        // then a jump every 20 seconds and nothing in between.
        let start = Instant::now();
        let mut guard = ThroughputGuard::new(Direction::Send, LIMITS, start, 0);
        let jump_interval = Duration::from_secs(20);
        let steps_per_jump = jump_interval.as_millis() / STEP.as_millis();
        let (mut now, mut total) = (start, 1_000_000);
        for step in 1..=steps_per_jump * 20 {
            now += STEP;
            if step % steps_per_jump == 0 {
                total += u64::from(MIN_RATE) * jump_interval.as_secs();
            }
            assert!(guard.check(now, total).is_ok(), "{:?}", now - start);
        }
    }

    #[test]
    fn should_not_cut_long_send_grace_period_to_max_credit() {
        let limits = TransferLimits {
            grace_period: MAX_SEND_CREDIT * 2,
            ..LIMITS
        };
        let start = Instant::now();
        let mut guard = ThroughputGuard::new(Direction::Send, limits, start, 0);
        let (elapsed, _) = run_until_dropped(&mut guard, start, 0, 0);
        assert_eq!(elapsed, limits.grace_period);
    }

    #[test]
    fn should_count_data_received_during_grace_period() {
        let start = Instant::now();
        let mut guard = ThroughputGuard::new(Direction::Receive, LIMITS, start, 0);
        // Enough for the whole first window arrives during the grace period, then nothing.
        let enough = u64::from(MIN_RATE) * GRACE.as_secs();
        assert!(guard.check(start + ms(100), enough).is_ok());
        // The first window, ending with the grace period, is satisfied by that data.
        assert!(guard.check(start + GRACE, enough).is_ok());
        // The next window is evaluated on its own, once it completes.
        assert!(guard.check(start + GRACE * 2 - ms(1), enough).is_ok());
        assert!(guard.check(start + GRACE * 2, enough).is_err());
    }

    #[test]
    fn should_credit_data_sent_during_grace_period() {
        let start = Instant::now();
        let mut guard = ThroughputGuard::new(Direction::Send, LIMITS, start, 0);
        let sent = u64::from(MIN_RATE) * 5;
        assert!(guard.check(start + ms(100), sent).is_ok());
        // The credit is added to the grace period.
        let deadline = start + GRACE + Duration::from_secs(5);
        assert!(guard.check(start + GRACE, sent).is_ok());
        assert!(guard.check(deadline - ms(1), sent).is_ok());
        assert!(guard.check(deadline, sent).is_err());
    }

    #[test]
    fn should_keep_transfer_meeting_minimum_rate() {
        for grace_period in [GRACE, ms(100)] {
            for direction in DIRECTIONS {
                let limits = TransferLimits {
                    grace_period,
                    ..LIMITS
                };
                let step = clamp_interval(grace_period);
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
            let step = clamp_interval(limits.grace_period);
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
