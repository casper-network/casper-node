//! Networking debug insights.
//!
//! The `insights` module exposes some internals of the networking component, mainly for inspection
//! through the diagnostics console. It should specifically not be used for any business logic and
//! affordances made in other corners of the `network` module to allow collecting these
//! insights should neither be abused just because they are available.

use std::{
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};

use casper_types::{EraId, PublicKey, TimeDiff};
use serde::Serialize;

use crate::{types::NodeId, utils::opt_display::OptDisplay};

use super::{
    conman::{Direction, Route},
    Network, Payload,
};

/// A collection of insights into the active networking component.
#[derive(Debug, Serialize)]
pub(crate) struct NetworkInsights {
    /// The nodes current ID.
    our_id: NodeId,
    /// Whether or not a network CA was present (is a private network).
    network_ca: bool,
    /// The public address of the node.
    public_addr: Option<SocketAddr>,
    /// The fingerprint of a consensus key installed.
    consensus_public_key: Option<PublicKey>,
    /// The active era as seen by the networking component.
    net_active_era: EraId,
    /// All active routes.
    active_routes: Vec<RouteInsights>,
}

/// Information about existing routes.
#[derive(Debug, Serialize)]
pub(crate) struct RouteInsights {
    /// Node ID of the peer.
    pub(crate) peer: NodeId,
    /// The remote address of the peer.
    pub(crate) remote_addr: SocketAddr,
    /// Incoming or outgoing?
    pub(crate) direction: Direction,
    /// The consensus key provided by the peer during handshake.
    pub(crate) consensus_key: Option<Arc<PublicKey>>,
    /// Duration since this route was established.
    pub(crate) since: TimeDiff,
}

impl RouteInsights {
    /// Creates a new instance from an existing `Route`.
    fn collect_from_route(now: Instant, route: &Route) -> Self {
        Self {
            peer: route.peer,
            remote_addr: route.remote_addr,
            direction: route.direction,
            consensus_key: route.consensus_key.clone(),
            since: now.duration_since(route.since).into(),
        }
    }
}

impl NetworkInsights {
    /// Collect networking insights from a given networking component.
    pub(super) fn collect_from_component<P>(net: &Network<P>) -> Self
    where
        P: Payload,
    {
        let mut active_routes = Vec::new();

        if let Some(ref conman) = net.conman {
            // Acquire lock only long enough to copy routing table.
            let guard = conman.read_state();
            let now = Instant::now();
            active_routes.extend(
                guard
                    .routing_table()
                    .values()
                    .map(|route| RouteInsights::collect_from_route(now, route)),
            );
        }

        // Sort only after releasing lock.
        active_routes.sort_by_key(|route_insight| route_insight.peer);

        NetworkInsights {
            our_id: net.our_id,
            network_ca: net.identity.network_ca.is_some(),
            public_addr: net.public_addr,
            consensus_public_key: net.node_key_pair.as_ref().map(|kp| kp.public_key().clone()),
            net_active_era: net.active_era,
            active_routes,
        }
    }
}

impl Display for RouteInsights {
    #[inline(always)]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} @ {} [{}] {}, since {}",
            self.peer,
            self.remote_addr,
            self.direction,
            OptDisplay::new(self.consensus_key.as_ref(), "no key provided"),
            self.since,
        )
    }
}

impl Display for NetworkInsights {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if !self.network_ca {
            f.write_str("Public ")?;
        } else {
            f.write_str("Private ")?;
        }
        writeln!(
            f,
            "node {} @ {}",
            self.our_id,
            OptDisplay::new(self.public_addr, "no listen addr")
        )?;

        write!(f, "in {} (according to networking), ", self.net_active_era)?;

        match self.consensus_public_key.as_ref() {
            Some(pub_key) => writeln!(f, "consensus pubkey {}", pub_key)?,
            None => f.write_str("no consensus key\n")?,
        }

        for route in &self.active_routes {
            writeln!(f, "{}", route)?;
        }

        Ok(())
    }
}
