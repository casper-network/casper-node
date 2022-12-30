//! Health-check state machine.
//!
//! Health checks perform periodic pings to remote peers to ensure the connection is still alive. It
//! has somewhat complicated logic that is encoded in the `ConnectionHealth` struct, which has
//! multiple implicit states.

use std::time::{Duration, Instant};

use datasize::DataSize;
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
    /// The nonce of the timestamp.
    pub nonce: Nonce,
    /// The actual timestamp.
    pub timestamp: Instant,
}

/// A number-used-once, specifically one used in pings.
#[derive(Clone, Copy, DataSize, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Nonce(u32);

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

    pub(crate) fn check_health(&self, cfg: &HealthConfig, now: Instant) -> HealthCheckOutcome {
        // Our honeymoon period is from first establishment of the connection until we send a ping.
        if now.duration_since(self.connected_since) < cfg.ping_interval {
            return HealthCheckOutcome::DoNothing;
        }

        todo!("remaining health check logic")
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
