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
    blocklist::BlocklistJustification,
    conman::{Direction, Route, Sentence},
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
    /// Addresses for which an outgoing task is currently running.
    address_book: Vec<SocketAddr>,
    /// Blocked addresses.
    do_not_call_list: Vec<DoNotCallInsight>,
    /// All active routes.
    active_routes: Vec<RouteInsight>,
    /// Bans currently active.
    blocked: Vec<SentenceInsight>,
}

/// Information about existing routes.
#[derive(Debug, Serialize)]
pub(crate) struct RouteInsight {
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

/// Information about an existing ban.
#[derive(Debug, Serialize)]
pub(crate) struct SentenceInsight {
    /// The peer banned.
    pub(crate) peer: NodeId,
    /// Time until the ban is lifted.
    pub(crate) remaining: Option<TimeDiff>,
    /// Justification for the ban.
    pub(crate) justification: BlocklistJustification,
}

/// Information about an entry of the do-not-call list.
#[derive(Debug, Serialize)]
pub(crate) struct DoNotCallInsight {
    /// Address not to be called.
    pub(crate) addr: SocketAddr,
    /// How long not to call the address.
    pub(crate) remaining: Option<TimeDiff>,
}

impl SentenceInsight {
    /// Creates a new instance from an existing `Route`.
    fn collect_from_sentence(now: Instant, peer: NodeId, sentence: &Sentence) -> Self {
        let remaining = if sentence.until > now {
            Some(sentence.until.duration_since(now).into())
        } else {
            None
        };
        Self {
            peer,
            remaining,
            justification: sentence.justification.clone(),
        }
    }
}

impl RouteInsight {
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

impl DoNotCallInsight {
    /// Creates a new instance from an existing entry on the do-not-call list.
    fn collect_from_dnc(now: Instant, addr: SocketAddr, until: Instant) -> Self {
        let remaining = if until > now {
            Some(until.duration_since(now).into())
        } else {
            None
        };

        DoNotCallInsight { addr, remaining }
    }
}

impl NetworkInsights {
    /// Collect networking insights from a given networking component.
    pub(super) fn collect_from_component<P>(net: &Network<P>) -> Self
    where
        P: Payload,
    {
        let mut address_book = Vec::new();
        let mut do_not_call_list = Vec::new();
        let mut active_routes = Vec::new();
        let mut blocked = Vec::new();

        if let Some(ref conman) = net.conman {
            // Acquire lock only long enough to copy routing table.
            let guard = conman.read_state();
            let now = Instant::now();
            address_book = guard.address_book().iter().cloned().collect();

            active_routes.extend(
                guard
                    .routing_table()
                    .values()
                    .map(|route| RouteInsight::collect_from_route(now, route)),
            );
            do_not_call_list.extend(
                guard
                    .do_not_call()
                    .iter()
                    .map(|(&addr, &until)| DoNotCallInsight::collect_from_dnc(now, addr, until)),
            );
            blocked.extend(guard.banlist().iter().map(|(&peer, sentence)| {
                SentenceInsight::collect_from_sentence(now, peer, sentence)
            }));
        }

        // Sort only after releasing lock.
        address_book.sort();
        do_not_call_list.sort_by_key(|dnc| dnc.addr);
        active_routes.sort_by_key(|route_insight| route_insight.peer);
        blocked.sort_by_key(|sentence_insight| sentence_insight.peer);

        NetworkInsights {
            our_id: net.our_id,
            network_ca: net.identity.network_ca.is_some(),
            public_addr: net.public_addr,
            consensus_public_key: net.node_key_pair.as_ref().map(|kp| kp.public_key().clone()),
            net_active_era: net.active_era,
            address_book,
            do_not_call_list,
            active_routes,
            blocked,
        }
    }
}

impl Display for DoNotCallInsight {
    #[inline(always)]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} for another {} ",
            self.addr,
            OptDisplay::new(self.remaining.as_ref(), "(expired)"),
        )
    }
}

impl Display for RouteInsight {
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

impl Display for SentenceInsight {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} for another {}: {}",
            self.peer,
            OptDisplay::new(self.remaining.as_ref(), "(expired)"),
            self.justification
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

        f.write_str("\naddress book:\n")?;

        for addr in &self.address_book {
            write!(f, "{} ", addr)?;
        }

        f.write_str("\n\ndo-not-call:\n")?;

        for dnc in &self.do_not_call_list {
            writeln!(f, "{}", dnc)?;
        }

        f.write_str("\nroutes:\n")?;

        for route in &self.active_routes {
            writeln!(f, "{}", route)?;
        }

        f.write_str("\nblocklist:\n")?;

        for sentence in &self.blocked {
            writeln!(f, "{}", sentence)?;
        }

        Ok(())
    }
}
