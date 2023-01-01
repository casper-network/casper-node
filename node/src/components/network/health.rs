//! Health-check state machine.
//!
//! Health checks perform periodic pings to remote peers to ensure the connection is still alive. It
//! has somewhat complicated logic that is encoded in the `ConnectionHealth` struct, which has
//! multiple implicit states.

use std::time::{Duration, Instant};

use datasize::DataSize;
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Connection health information.
///
/// All data related to the ping/pong functionality used to verify a peer's networking liveness.
#[derive(Clone, Copy, DataSize, Debug)]
pub(crate) struct ConnectionHealth {
    /// The moment the connection was established.
    pub(crate) connected_since: Instant,
    /// The last ping that was requested to be sent.
    pub(crate) last_ping_sent: Option<TaggedTimestamp>,
    /// The most recent pong received.
    pub(crate) last_pong_received: Option<TaggedTimestamp>,
    /// Number of invalid pongs received, reset upon receiving a valid pong.
    pub(crate) invalid_pong_count: u32,
    /// Number of pings that timed out.
    pub(crate) ping_timeouts: u32,
}

/// Health check configuration.
#[derive(DataSize, Debug)]
pub(crate) struct HealthConfig {
    /// How often to send a ping to ensure a connection is established.
    ///
    /// The interval determines how soon after connecting or a successful ping another ping is sent.
    pub(crate) ping_interval: Duration,
    /// Duration during which a ping must succeed to be considered successful.
    pub(crate) ping_timeout: Duration,
    /// Number of attempts before giving up and disconnecting a peer due to too many failed pings.
    pub(crate) ping_retries: u16,
    /// How many spurious pongs to tolerate before banning a peer.
    pub(crate) pong_limit: u32,
}

/// A timestamp with an associated nonce.
#[derive(Clone, Copy, DataSize, Debug)]
pub(crate) struct TaggedTimestamp {
    /// The actual timestamp.
    timestamp: Instant,
    /// The nonce of the timestamp.
    nonce: Nonce,
}

impl TaggedTimestamp {
    /// Creates a new tagged timestamp with a random nonce.
    pub(crate) fn new<R: Rng>(rng: &mut R, timestamp: Instant) -> Self {
        Self {
            timestamp,
            nonce: rng.gen(),
        }
    }

    /// Creates a new tagged timestamp from parts.
    #[cfg(test)]
    pub(crate) fn from_parts(timestamp: Instant, nonce: Nonce) -> Self {
        TaggedTimestamp { nonce, timestamp }
    }

    /// Returns the actual timestamp.
    pub(crate) fn timestamp(&self) -> Instant {
        self.timestamp
    }

    /// Returns the nonce inside the timestamp.
    pub(crate) fn nonce(self) -> Nonce {
        self.nonce
    }
}

/// A number-used-once, specifically one used in pings.
// Note: This nonce used to be a `u32`, but that is too small - since we immediately disconnect when
//       a duplicate ping is generated, a `u32` has a ~ 1/(2^32) chance of a consecutive collision.
//
//       If we ping every 5 seconds, this is a ~ 0.01% chance over a month, which is too high over
//       thousands over nodes. At 64 bits, in theory the upper bound is 0.0000000002%, which is
//       better (the period of the RNG used should be >> 64 bits).
//
//       While we do check for consecutive ping nonces being generated, we still like the lower
//       collision chance for repeated pings being sent.
#[derive(Clone, Copy, DataSize, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub(crate) struct Nonce(u64);

impl rand::distributions::Distribution<Nonce> for rand::distributions::Standard {
    #[inline(always)]
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Nonce {
        Nonce(rng.gen())
    }
}

impl ConnectionHealth {
    /// Creates a new connection health instance, recording when the connection was established.
    pub(crate) fn new(connected_since: Instant) -> Self {
        Self {
            connected_since,
            last_ping_sent: None,
            last_pong_received: None,
            invalid_pong_count: 0,
            ping_timeouts: 0,
        }
    }
}

impl ConnectionHealth {
    /// Calculate the round-trip time, if possible.
    pub(crate) fn calc_rrt(&self) -> Option<Duration> {
        match (self.last_ping_sent, self.last_pong_received) {
            (Some(last_ping), Some(last_pong)) if last_ping.nonce == last_pong.nonce => {
                Some(last_pong.timestamp.duration_since(last_ping.timestamp))
            }
            _ => None,
        }
    }

    /// Check current health status.
    ///
    /// This function must be polled periodically and returns a potential action to be performed.
    pub(crate) fn update_health<R: Rng>(
        &mut self,
        rng: &mut R,
        cfg: &HealthConfig,
        now: Instant,
    ) -> HealthCheckOutcome {
        // Our honeymoon period is from first establishment of the connection until we send a ping.
        if now.duration_since(self.connected_since) < cfg.ping_interval {
            return HealthCheckOutcome::DoNothing;
        }

        let send_ping = match self.last_ping_sent {
            Some(last_ping) => {
                match self.last_pong_received {
                    Some(prev_pong) if prev_pong.nonce() == last_ping.nonce() => {
                        // Normal operation. The next ping should be sent in a regular interval
                        // after receiving the last pong.
                        now >= prev_pong.timestamp() + cfg.ping_interval
                    }

                    _ => {
                        // No matching pong on record. Check if we need to timeout the ping.
                        if now >= last_ping.timestamp() + cfg.ping_timeout {
                            self.ping_timeouts += 1;
                            // Clear the `last_ping_sent`, schedule another to be sent.
                            self.last_ping_sent = None;
                            true
                        } else {
                            false
                        }
                    }
                }
            }
            None => true,
        };

        if send_ping {
            if self.ping_timeouts > cfg.ping_retries as u32 {
                // We have exceeded the timeouts and will give up as a result.
                return HealthCheckOutcome::GiveUp;
            }

            let ping = loop {
                let candidate = TaggedTimestamp::new(rng, now);

                if let Some(prev) = self.last_ping_sent {
                    if prev.nonce() == candidate.nonce() {
                        // Ensure we don't produce consecutive pings.
                        continue;
                    }
                }

                break candidate;
            };

            self.last_ping_sent = Some(ping);
            HealthCheckOutcome::SendPing(ping.nonce())
        } else {
            HealthCheckOutcome::DoNothing
        }
    }

    /// Records a pong that has been sent.
    ///
    /// If `true`, the maximum number of pongs has been exceeded and the peer should be banned.
    pub(crate) fn record_pong(&mut self, cfg: &HealthConfig, tt: TaggedTimestamp) -> bool {
        let is_valid_pong = match self.last_ping_sent {
            Some(last_ping) if last_ping.nonce() == tt.nonce => {
                // Check if we already received a pong for this ping, which is a protocol violation.
                if self
                    .last_pong_received
                    .map(|existing| existing.nonce() == tt.nonce)
                    .unwrap_or(false)
                {
                    // Ping is a collsion, ban.
                    return true;
                }

                if last_ping.timestamp() > tt.timestamp() {
                    // Ping is from the past somehow, ignore it (probably a bug on our side).
                    return false;
                }

                // The ping is valid if it is within the timeout period.
                last_ping.timestamp() + cfg.ping_timeout >= tt.timestamp()
            }
            _ => {
                // Either the nonce did not match, or the nonce mismatched.
                false
            }
        };

        if is_valid_pong {
            // Our pong is valid, reset invalid count and record it.
            self.invalid_pong_count = 0;
            self.last_pong_received = Some(tt);
            false
        } else {
            self.invalid_pong_count += 1;
            // If we have exceeded the invalid pong limit, ban.
            self.invalid_pong_count > cfg.pong_limit
        }
    }
}

/// The outcome of periodic health check.
#[derive(Clone, Copy, Debug)]

pub(crate) enum HealthCheckOutcome {
    /// Do nothing, as we recently took action.
    DoNothing,
    /// Send a ping with the given nonce.
    SendPing(Nonce),
    /// Give up on (i.e. terminate) the connection, as we exceeded the allowable ping limit.
    GiveUp,
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, time::Duration};

    use assert_matches::assert_matches;
    use rand::Rng;

    use super::{ConnectionHealth, HealthCheckOutcome, HealthConfig};
    use crate::{
        components::network::health::TaggedTimestamp, testing::test_clock::TestClock,
        types::NodeRng,
    };

    impl HealthConfig {
        pub(crate) fn test_config() -> Self {
            // Note: These values are assumed in tests, so do not change them.
            HealthConfig {
                ping_interval: Duration::from_secs(5),
                ping_timeout: Duration::from_secs(2),
                ping_retries: 3,
                pong_limit: 6,
            }
        }
    }

    struct Fixtures {
        clock: TestClock,
        cfg: HealthConfig,
        rng: NodeRng,
        health: ConnectionHealth,
    }

    /// Sets up fixtures used in almost every test.
    fn fixtures() -> Fixtures {
        let mut clock = TestClock::new();
        let cfg = HealthConfig::test_config();
        let mut rng = crate::new_rng();

        let health = ConnectionHealth::new(clock.now());

        Fixtures {
            clock,
            cfg,
            rng,
            health,
        }
    }

    #[test]
    fn scenario_no_response() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // Repeated checks should not change the outcome.
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // After 4.9 seconds, we still do not send a ping.
        clock.advance(Duration::from_millis(4900));

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // At 5, we expect our first ping.
        clock.advance(Duration::from_millis(100));

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // Checking health again should not result in another ping.
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        clock.advance(Duration::from_millis(100));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // After two seconds, we expect another ping to be sent, due to timeouts.
        clock.advance(Duration::from_millis(2000));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // At this point, two pings have been sent. Configuration says to retry 3 times, so a total
        // of five pings is expected.
        clock.advance(Duration::from_millis(2000));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        clock.advance(Duration::from_millis(2000));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // Finally, without receiving a ping at all, we give up.
        clock.advance(Duration::from_millis(2000));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::GiveUp
        );
    }

    #[test]
    fn pings_use_different_nonces() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );
        clock.advance(Duration::from_secs(5));

        let mut nonce_set = HashSet::new();

        nonce_set.insert(assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        ));
        clock.advance(Duration::from_secs(2));

        nonce_set.insert(assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        ));
        clock.advance(Duration::from_secs(2));

        nonce_set.insert(assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        ));
        clock.advance(Duration::from_secs(2));

        nonce_set.insert(assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        ));

        // Since it is a set, we expect less than 4 items if there were any duplicates.
        assert_eq!(nonce_set.len(), 4);
    }

    #[test]
    fn scenario_all_working() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // At 5 seconds, we expect our first ping.
        clock.advance(Duration::from_secs(5));

        let nonce_1 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        // Record a reply 500 ms later.
        clock.advance(Duration::from_millis(500));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_1)));

        // Our next pong should be 5 seconds later, not 4.5.
        clock.advance(Duration::from_millis(4500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );
        clock.advance(Duration::from_millis(500));

        let nonce_2 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        // We test an edge case here where we use the same timestamp for the received pong.
        clock.advance(Duration::from_millis(500));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_2)));

        // Afterwards, no ping should be sent.
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // Do 1000 additional ping/pongs.
        for _ in 0..1000 {
            clock.advance(Duration::from_millis(5000));
            let nonce = assert_matches!(
                health.update_health(&mut rng, &cfg, clock.now()),
                HealthCheckOutcome::SendPing(nonce) => nonce
            );
            assert_matches!(
                health.update_health(&mut rng, &cfg, clock.now()),
                HealthCheckOutcome::DoNothing
            );

            clock.advance(Duration::from_millis(250));
            assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce)));

            assert_matches!(
                health.update_health(&mut rng, &cfg, clock.now()),
                HealthCheckOutcome::DoNothing
            );
        }
    }

    #[test]
    fn scenario_intermittent_failures() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        // We miss two pings initially, before recovering.
        clock.advance(Duration::from_secs(5));

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        clock.advance(Duration::from_secs(2));

        let nonce_1 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        clock.advance(Duration::from_secs(1));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_1)));

        // We successfully "recovered", this should reset our ping counts. Miss three pings before
        // successfully receiving a pong from 4th from here on out.
        clock.advance(Duration::from_millis(5500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        let nonce_2 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );
        clock.advance(Duration::from_millis(500));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_2)));

        // This again should reset. We miss four more pings and are disconnected.
        clock.advance(Duration::from_millis(5500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
        clock.advance(Duration::from_millis(2500));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::GiveUp
        );
    }

    #[test]
    fn ignores_unwanted_pongs() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        clock.advance(Duration::from_secs(5));

        let nonce_1 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        // We have received `nonce_1`. Make the `ConnectionHealth` receive some unasked pongs,
        // without exceeding the unasked pong limit.
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));

        // The retry delay is 2 seconds (instead of 5 for the next pong after success), so ensure
        // we retry due to not having received the correct nonce in the pong.

        clock.advance(Duration::from_secs(2));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
    }

    #[test]
    fn ensure_excessive_pongs_result_in_ban() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        clock.advance(Duration::from_secs(5));

        let nonce_1 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        // We have received `nonce_1`. Make the `ConnectionHealth` receive some unasked pongs,
        // without exceeding the unasked pong limit.
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        // 6 unasked pongs is still okay.

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        assert!(health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));
        // 7 is too much.

        // For good measure, we expect the health check to also output a disconnect instruction.
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::GiveUp
        );
    }

    #[test]
    fn time_reversal_does_not_crash_but_is_ignored() {
        // Usually a pong for a given (or any) nonce should always be received with a timestamp
        // equal or later than the ping sent out. Due to a programming error or a lucky attacker +
        // scheduling issue, there is a very minute chance this can actually happen.
        //
        // In these cases, the pongs should just be discarded, not crashing due to a underflow in
        // the comparison.
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        clock.advance(Duration::from_secs(5));

        let nonce_1 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        // Ignore the nonce if sent in the past (and also don't crash).
        clock.rewind(Duration::from_secs(1));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_1)));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), rng.gen())));

        // Another ping should be sent out, since `nonce_1` was ignored.
        clock.advance(Duration::from_secs(2));
        let nonce_2 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        // Nonce 2 will be received seemingly before the connection was even established.
        clock.rewind(Duration::from_secs(3600));
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_2)));
    }

    #[test]
    fn handles_missed_health_checks() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        clock.advance(Duration::from_secs(15));

        // We initially exceed our scheduled first ping by 10 seconds. This will cause the ping to
        // be sent right there and then.
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // Going forward 1 second should not change anything.
        clock.advance(Duration::from_secs(1));

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        // After another second, two seconds have passed since sending the first ping in total, so
        // send another once.
        clock.advance(Duration::from_secs(1));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // We have missed two pings total, now wait an hour. This will trigger the third ping.
        clock.advance(Duration::from_secs(3600));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // Fourth right after
        clock.advance(Duration::from_secs(2));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        // Followed by a disconnect.
        clock.advance(Duration::from_secs(2));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::GiveUp
        );
    }

    #[test]
    fn ignores_time_travel() {
        // Any call of the health update with timestamps that are provably from the past (i.e.
        // before a recorded timestamp like a previous ping) should be ignored.

        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        clock.advance(Duration::from_secs(5));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );

        clock.rewind(Duration::from_secs(3));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );

        clock.advance(Duration::from_secs(6));
        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::DoNothing
        );
        clock.advance(Duration::from_secs(1));

        assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(_)
        );
    }

    #[test]
    fn duplicate_pong_immediately_terminates() {
        let Fixtures {
            mut clock,
            cfg,
            mut rng,
            mut health,
        } = fixtures();

        clock.advance(Duration::from_secs(5));
        let nonce_1 = assert_matches!(
            health.update_health(&mut rng, &cfg, clock.now()),
            HealthCheckOutcome::SendPing(nonce) => nonce
        );

        clock.advance(Duration::from_secs(1));

        // Recording the pong once is fine, but the second time should result in a ban.
        assert!(!health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_1)));
        assert!(health.record_pong(&cfg, TaggedTimestamp::from_parts(clock.now(), nonce_1)));
    }
}
