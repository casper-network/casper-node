use casper_types::{TimeDiff, Timestamp};
use std::time::Duration;
use tokio::{select, sync::Mutex, time};
use tokio_util::sync::CancellationToken;

struct TerminationData {
    /// Moment in time at which the termination will happen. The
    /// actual termination can happen some time after this
    /// timestamp within reasonable timeframe of waking up
    /// threads and rusts internal polling mechanisms
    terminate_at: Timestamp,
    /// A cancellation token which can stop (by calling
    /// `.cancel()` on it) the countdown in case an extended
    /// lifetime needs to be placed
    stop_countdown: CancellationToken,
}

/// Terminator which causes a cancellation_token to get canceled if a given timeout occurs.
/// Allows to extend the timeout period by resetting the termination dealine (using `terminate_at`)
/// or with a helper function `delay_by`. Both functions won't reset the termination deadline if
/// the new termination would happen before the existing one (we only allow to extend the
/// termination period)
pub(super) struct ConnectionTerminator {
    /// This token will get canceled if the timeout passes
    cancellation_token: CancellationToken,
    //Data steering the internal countdown
    countdown_data: Mutex<Option<TerminationData>>,
}

impl ConnectionTerminator {
    /// Updates or sets the termination deadline.
    /// There will be no update if the termination already happened.
    /// Both set and update won't happen if the `in_terminate_at` is in the past.
    /// Updating an already running termination countdown happens only if the incomming
    /// `in_terminate_at` is > then the existing one. Returns true if the update was in effect.
    /// False otherwise
    pub(super) async fn terminate_at(&self, in_terminate_at: Timestamp) -> bool {
        let now = Timestamp::now();
        if in_terminate_at <= now {
            //Do nothing if termiantion is in the past
            return false;
        }
        let terminate_in = Duration::from_millis(in_terminate_at.millis() - now.millis());
        let mut countdown_data_guard = self.countdown_data.lock().await;
        if let Some(TerminationData {
            terminate_at,
            stop_countdown,
        }) = countdown_data_guard.as_ref()
        {
            if in_terminate_at < *terminate_at {
                //Don't update termination time if the proposed one is more restrictive than
                // the existing one.
                return false;
            } else {
                stop_countdown.cancel();
            }
        }
        if self.cancellation_token.is_cancelled() {
            //Don't proceed if the outbound token was already cancelled
            return false;
        }
        let stop_countdown = self
            .spawn_termination_countdown(terminate_in, self.cancellation_token.clone())
            .await;
        let data = TerminationData {
            terminate_at: in_terminate_at,
            stop_countdown,
        };
        *countdown_data_guard = Some(data);
        true
    }

    /// Delays the termination by `delay_by` amount. If the terminations `terminate_at` is
    /// further in the future than `now() + delay_by`, this function will have no effect
    /// and will return false. Returns true otherwise.
    pub(crate) async fn delay_termination(&self, delay_by: TimeDiff) -> bool {
        let temrinate_at = Timestamp::now() + delay_by;
        self.terminate_at(temrinate_at).await
    }

    //Ctor. To start the countdown mechanism you need to call `terminate_at`
    pub(super) fn new() -> Self {
        let cancellation_token = CancellationToken::new();
        ConnectionTerminator {
            cancellation_token,
            countdown_data: Mutex::new(None),
        }
    }

    pub(super) fn get_cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    // Spawns a thread that will cancel `cancellation_token` in a given `terminate_in` duration.
    // This function doesn't check if the cancellation_token wasn't already cancelled - it needs to
    // be checked beforehand Return a different CancellationToken which can be used to kill the
    // running thread
    async fn spawn_termination_countdown(
        &self,
        terminate_in: Duration,
        cancellation_token: CancellationToken,
    ) -> CancellationToken {
        let cancel_countdown = CancellationToken::new();
        let cancel_countdown_to_move = cancel_countdown.clone();
        tokio::task::spawn(async move {
            select! {
                _ = time::sleep(terminate_in) => {
                    cancellation_token.cancel()
                },
                _ = cancel_countdown_to_move.cancelled() => {
                },

            }
        });
        cancel_countdown
    }
}

#[cfg(test)]
mod tests {
    use super::ConnectionTerminator;
    use casper_types::{TimeDiff, Timestamp};
    use std::time::Duration;
    use tokio::{select, time::sleep};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn should_fail_setting_expiration_in_past() {
        let terminator = ConnectionTerminator::new();
        let in_past = Timestamp::from(1);
        assert!(!terminator.terminate_at(in_past).await);

        let initial_inactivity = TimeDiff::from_seconds(1);
        let terminator = ConnectionTerminator::new();
        let now = Timestamp::now();
        assert!(terminator.terminate_at(now + initial_inactivity).await);
        assert!(!terminator.terminate_at(now).await);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn should_fail_setting_expiration_when_already_cancelled() {
        let initial_inactivity = TimeDiff::from_seconds(1);
        let terminator = ConnectionTerminator::new();
        let now = Timestamp::now();
        assert!(terminator.terminate_at(now + initial_inactivity).await);
        let cancellation_token = terminator.get_cancellation_token();
        select! {
            _ = cancellation_token.cancelled() => {
                let elapsed = now.elapsed();
                assert!(elapsed >= TimeDiff::from_seconds(1));
                assert!(elapsed <= TimeDiff::from_millis(1500));
            },
            _ = sleep(Duration::from_secs(10)) => {
                unreachable!()
            },
        }

        let initial_inactivity = TimeDiff::from_seconds(10);
        let now = Timestamp::now();
        assert!(!terminator.terminate_at(now + initial_inactivity).await);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn should_cancel_after_enough_inactivity() {
        let initial_inactivity = TimeDiff::from_seconds(1);
        let terminator = ConnectionTerminator::new();
        let now = Timestamp::now();
        assert!(terminator.terminate_at(now + initial_inactivity).await);
        let cancellation_token = terminator.get_cancellation_token();
        select! {
            _ = cancellation_token.cancelled() => {
                let elapsed = now.elapsed();
                assert!(elapsed >= TimeDiff::from_seconds(1));
                assert!(elapsed <= TimeDiff::from_millis(1500));
            },
            _ = sleep(Duration::from_secs(10)) => {
                unreachable!()
            },
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn should_cancel_after_extended_time() {
        let initial_inactivity = TimeDiff::from_seconds(1);
        let terminator = ConnectionTerminator::new();
        let now = Timestamp::now();
        assert!(terminator.terminate_at(now + initial_inactivity).await);
        sleep(Duration::from_millis(100)).await;
        terminator
            .delay_termination(TimeDiff::from_seconds(2))
            .await;
        let cancellation_token = terminator.get_cancellation_token();
        select! {
            _ = cancellation_token.cancelled() => {
                let elapsed = now.elapsed();
                assert!(elapsed >= TimeDiff::from_seconds(2));
                assert!(elapsed <= TimeDiff::from_millis(2500));
            },
            _ = sleep(Duration::from_secs(10)) => {
                unreachable!()
            },
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn should_cancel_after_multiple_time_extensions() {
        let initial_inactivity = TimeDiff::from_seconds(1);
        let terminator = ConnectionTerminator::new();
        let now = Timestamp::now();
        assert!(terminator.terminate_at(now + initial_inactivity).await);
        sleep(Duration::from_millis(100)).await;
        terminator
            .delay_termination(TimeDiff::from_seconds(2))
            .await;
        sleep(Duration::from_millis(100)).await;
        terminator
            .delay_termination(TimeDiff::from_seconds(3))
            .await;
        let cancellation_token = terminator.get_cancellation_token();
        select! {
            _ = cancellation_token.cancelled() => {
                let elapsed = now.elapsed();
                assert!(elapsed >= TimeDiff::from_seconds(3));
                assert!(elapsed <= TimeDiff::from_millis(4000));
            },
            _ = sleep(Duration::from_secs(10)) => {
                unreachable!()
            },
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn should_not_shorten_termination_time() {
        let initial_inactivity = TimeDiff::from_seconds(1);
        let terminator = ConnectionTerminator::new();
        let now = Timestamp::now();
        assert!(terminator.terminate_at(now + initial_inactivity).await);
        sleep(Duration::from_millis(100)).await;
        terminator
            .delay_termination(TimeDiff::from_seconds(2))
            .await;
        sleep(Duration::from_millis(100)).await;
        terminator
            .delay_termination(TimeDiff::from_seconds(1))
            .await;
        let cancellation_token = terminator.get_cancellation_token();
        select! {
            _ = cancellation_token.cancelled() => {
                let elapsed = now.elapsed();
                assert!(elapsed >= TimeDiff::from_seconds(2));
                assert!(elapsed <= TimeDiff::from_millis(2500));
            },
            _ = sleep(Duration::from_secs(10)) => {
                unreachable!()
            },
        }
    }
}
