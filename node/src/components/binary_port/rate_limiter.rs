use casper_types::{TimeDiff, Timestamp};
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub(crate) enum RateLimiterError {
    #[error("Cannot create Rate limiter with 0 max_requests")]
    EmptyWindowNotAllowed,
    #[error("Maximum window duration is too large")]
    WindowDurationTooLarge,
    #[error("Maximum window duration is too small")]
    WindowDurationTooSmall,
}

const MAX_WINDOW_DURATION_MS: u64 = 1000 * 60 * 60; // 1 hour

#[derive(PartialEq, Eq, Debug)]
/// Response from the rate limiter.
pub(crate) enum LimiterResponse {
    /// when limiter allowed the request
    Allowed,
    /// when limiter throttled the request
    Throttled,
}

/// A buffer to store timestamps of requests. The assumption is that the buffer will keep the
/// monotonical order of timestamps as they are pushed.
#[derive(Debug)]
struct Buffer {
    buffer: Vec<u64>,
    in_index: usize,
    out_index: usize,
    capacity: usize,
}

impl Buffer {
    fn new(size: usize) -> Self {
        Buffer {
            buffer: vec![0; size + 1],
            in_index: 0,
            out_index: 0,
            capacity: size + 1,
        }
    }

    fn is_full(&self) -> bool {
        self.in_index == (self.out_index + self.capacity - 1) % self.capacity
    }

    fn is_empty(&self) -> bool {
        self.in_index == self.out_index
    }

    //This should only be used from `push`
    fn push_and_slide(&mut self, value: u64) -> bool {
        let out_index = self.out_index as i32;
        let capacity = self.capacity as i32;
        let mut to_index = self.in_index as i32;
        let mut from_index = (self.in_index as i32 + capacity - 1) % capacity;

        while to_index != out_index && self.buffer[from_index as usize] > value {
            self.buffer[to_index as usize] = self.buffer[from_index as usize];
            to_index = (to_index + capacity - 1) % capacity;
            from_index = (from_index + capacity - 1) % capacity;
        }
        self.buffer[to_index as usize] = value;
        self.in_index = (self.in_index + 1) % self.capacity;
        true
    }

    fn push(&mut self, value: u64) -> bool {
        if self.is_full() {
            return false;
        }
        if !self.is_empty() {
            let last_stored_index = (self.in_index + self.capacity - 1) % self.capacity;
            let last_stored = self.buffer[last_stored_index];
            // We are expecting values to be monotonically increasing. But there is a scenario in
            // which the system time might be changed to a previous time.
            // We handle that by wiggling it inside the buffer
            if last_stored > value {
                return self.push_and_slide(value);
            }
        }
        self.buffer[self.in_index] = value;
        self.in_index = (self.in_index + 1) % self.capacity;
        true
    }

    fn prune_lt(&mut self, value: u64) -> usize {
        if self.is_empty() {
            return 0;
        }
        let mut number_of_pruned = 0;
        while self.in_index != self.out_index {
            if self.buffer[self.out_index] >= value {
                break;
            }
            self.out_index = (self.out_index + 1) % self.capacity;
            number_of_pruned += 1;
        }
        number_of_pruned
    }

    #[cfg(test)]
    fn to_vec(&self) -> Vec<u64> {
        let mut vec = Vec::new();
        let mut local_out = self.out_index;
        while self.in_index != local_out {
            vec.push(self.buffer[local_out]);
            local_out = (local_out + 1) % self.capacity;
        }
        vec
    }
}

#[derive(Debug)]
pub(crate) struct RateLimiter {
    /// window duration.
    window_ms: u64,
    /// Log of unix epoch time in ms when requests were made.
    buffer: Buffer,
}

impl RateLimiter {
    //ctor
    pub(crate) fn new(
        max_requests: usize,
        window_duration: TimeDiff,
    ) -> Result<Self, RateLimiterError> {
        if max_requests == 0 {
            // We consider 0-max_requests as a misconfiguration
            return Err(RateLimiterError::EmptyWindowNotAllowed);
        }
        let window_duration_in_ms = window_duration.millis();
        if window_duration_in_ms >= MAX_WINDOW_DURATION_MS {
            return Err(RateLimiterError::WindowDurationTooLarge);
        }
        let window_duration_in_ms = window_duration.millis();
        if window_duration_in_ms == 0 {
            return Err(RateLimiterError::WindowDurationTooSmall);
        }
        Ok(RateLimiter {
            window_ms: window_duration_in_ms,
            buffer: Buffer::new(max_requests),
        })
    }

    pub(crate) fn throttle(&mut self) -> LimiterResponse {
        self.internal_throttle(Timestamp::now().millis())
    }

    fn internal_throttle(&mut self, now: u64) -> LimiterResponse {
        let is_full = self.buffer.is_full();
        if !is_full {
            self.buffer.push(now);
            return LimiterResponse::Allowed;
        } else {
            //The following subtraction could theoretically not fit in unsigned, but in real-life
            // cases we limit the window duration to 1 hour (it's checked in ctor). So unless
            // someone calls it from the perspective of 1970, it should be fine.
            let no_of_pruned = self.buffer.prune_lt(now - self.window_ms);
            if no_of_pruned == 0 {
                //No pruning was done, so we are still at max_requests
                return LimiterResponse::Throttled;
            }
        }
        self.buffer.push(now);
        LimiterResponse::Allowed
    }
}

#[cfg(test)]
mod tests {
    use casper_types::TimeDiff;

    use super::*;

    #[test]
    fn sliding_window_should_validate_ctor_inputs() {
        assert!(RateLimiter::new(0, TimeDiff::from_millis(1000)).is_err());
        assert!(RateLimiter::new(10, TimeDiff::from_millis(MAX_WINDOW_DURATION_MS + 1)).is_err());
        assert!(RateLimiter::new(10, TimeDiff::from_millis(0)).is_err());
    }

    #[test]
    fn sliding_window_throttle_should_limit_requests() {
        let mut rate_limiter = rate_limiter();
        let t_1 = 10000_u64;
        let t_2 = 10002_u64;
        let t_3 = 10003_u64;

        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_2),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_3),
            LimiterResponse::Throttled
        );
    }

    #[test]
    fn sliding_window_throttle_should_limit_requests_on_burst() {
        let mut rate_limiter = rate_limiter();
        let t_1 = 10000;
        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Throttled
        );
    }

    #[test]
    fn sliding_window_should_slide_away_from_old_checks() {
        let mut rate_limiter = rate_limiter();
        let t_1 = 10000_u64;
        let t_2 = 10002_u64;
        let t_3 = 11002_u64;
        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_2),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_3),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_3),
            LimiterResponse::Throttled
        );
    }

    #[test]
    fn sliding_window_should_take_past_timestamp() {
        let mut rate_limiter = rate_limiter();
        let t_1 = 10000_u64;
        let t_2 = 9999_u64;
        let t_3 = 10001_u64;
        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_2),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_3),
            LimiterResponse::Throttled
        );
    }

    #[test]
    fn sliding_window_should_anneal_timestamp_from_past_() {
        let mut rate_limiter = rate_limiter();
        let t_1 = 10000_u64;
        let t_2 = 9999_u64;
        let t_3 = 12001_u64;
        let t_4 = 12002_u64;
        assert_eq!(
            rate_limiter.internal_throttle(t_1),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_2),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_3),
            LimiterResponse::Allowed
        );
        assert_eq!(
            rate_limiter.internal_throttle(t_4),
            LimiterResponse::Allowed
        );
    }

    #[test]
    fn buffer_should_saturate_with_values() {
        let mut buffer = Buffer::new(3);
        assert!(buffer.push(1));
        assert!(buffer.push(2));
        assert!(buffer.push(3));
        assert!(!buffer.push(4));
        assert_eq!(buffer.to_vec(), vec![1_u64, 2_u64, 3_u64]);
    }

    #[test]
    fn buffer_should_prune() {
        let mut buffer = Buffer::new(3);
        assert!(buffer.push(1));
        assert!(buffer.push(2));
        assert!(buffer.push(3));
        assert_eq!(buffer.prune_lt(3), 2);
        assert!(buffer.push(4));
        assert_eq!(buffer.to_vec(), vec![3_u64, 4_u64]);
        assert_eq!(buffer.prune_lt(5), 2);

        assert!(buffer.push(1));
        assert!(buffer.push(2));
        assert!(buffer.push(3));
        assert_eq!(buffer.prune_lt(5), 3);
        assert!(buffer.to_vec().is_empty());

        assert!(buffer.push(5));
        assert!(buffer.push(6));
        assert!(buffer.push(7));
        assert_eq!(buffer.to_vec(), vec![5, 6, 7]);
    }

    #[test]
    fn push_and_slide_should_keep_order() {
        let mut buffer = Buffer::new(5);
        assert!(buffer.push(1));
        assert!(buffer.push(2));
        assert!(buffer.push(7));
        assert!(buffer.push(6));
        assert_eq!(buffer.to_vec(), vec![1, 2, 6, 7]);
        assert_eq!(buffer.prune_lt(7), 3);
        assert_eq!(buffer.to_vec(), vec![7]);

        let mut buffer = Buffer::new(4);
        assert!(buffer.push(2));
        assert!(buffer.push(8));
        assert!(buffer.push(5));
        assert!(buffer.push(1));
        assert_eq!(buffer.to_vec(), vec![1, 2, 5, 8]);
        assert_eq!(buffer.prune_lt(5), 2);
        assert_eq!(buffer.to_vec(), vec![5, 8]);

        let mut buffer = Buffer::new(4);
        assert!(buffer.push(2));
        assert!(buffer.push(8));
        assert!(buffer.push(2));
        assert!(buffer.push(1));
        assert_eq!(buffer.to_vec(), vec![1, 2, 2, 8]);

        let mut buffer = Buffer::new(4);
        assert!(buffer.push(2));
        assert!(buffer.push(8));
        assert!(buffer.push(3));
        assert!(buffer.push(1));
        assert_eq!(buffer.prune_lt(2), 1);
        assert!(buffer.push(0));
        assert_eq!(buffer.to_vec(), vec![0, 2, 3, 8]);

        let mut buffer = Buffer::new(4);
        assert!(buffer.push(8));
        assert!(buffer.push(7));
        assert!(buffer.push(6));
        assert!(buffer.push(5));
        assert_eq!(buffer.prune_lt(7), 2);
        assert!(buffer.push(9));
        assert!(buffer.push(10));
        assert_eq!(buffer.prune_lt(9), 2);
        assert!(buffer.push(11));
        assert!(buffer.push(1));
        assert_eq!(buffer.to_vec(), vec![1, 9, 10, 11]);
    }

    fn rate_limiter() -> RateLimiter {
        RateLimiter::new(2, TimeDiff::from_millis(1000)).unwrap()
    }
}
