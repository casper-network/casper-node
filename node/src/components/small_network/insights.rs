//! Networking debug insights.
//!
//! The `insights` module exposes some internals of the networking component, mainly for inspection
//! through the diagnostics console. It should specifically not be used for any business logic and
//! affordances made in other corners of the `small_network` module to allow collecting these
//! insights should neither be abused just because they are available.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
    sync::atomic::Ordering,
    time::SystemTime,
};

use casper_types::{EraId, PublicKey};
use serde::Serialize;

use crate::{types::NodeId, utils::TimeAnchor};

use super::{
    error::ConnectionError, message::ConsensusKeyPair, outgoing::OutgoingState,
    symmetry::ConnectionSymmetry, OutgoingHandle, Payload, SmallNetwork,
};

/// A collection of insights into the active networking component.
#[derive(Debug, Serialize)]
pub(crate) struct NetworkInsights {
    /// The nodes current ID.
    our_id: NodeId,
    /// Whether or not a network CA was present (is a private network).
    network_ca: bool,
    /// The public address of the node.
    public_addr: SocketAddr,
    /// The fingerprint of a consensus key installed.
    consensus_keys: Option<PublicKey>,
    /// Whether or not the node is syncing.
    is_syncing: bool,
    /// The active era as seen by the networking component.
    net_active_era: EraId,
    /// The list of node IDs that are being preferred due to being active validators.
    priviledged_active_outgoing_nodes: Option<HashSet<PublicKey>>,
    /// The list of node IDs that are being preferred due to being upcoming validators.
    priviledged_upcoming_outgoing_nodes: Option<HashSet<PublicKey>>,
    /// The amount of bandwidth allowance currently buffered, ready to be spent.
    unspent_bandwidth_allowance_bytes: Option<i64>,
    /// Map of outgoing connections, along with their current state.
    outgoing_connections: HashMap<SocketAddr, OutgoingInsight>,
    /// Map of incoming connections.
    connection_symmetries: HashMap<NodeId, ConnectionSymmetryInsight>,
}

/// Insight into an outgoing connection.
#[derive(Debug, Serialize)]
struct OutgoingInsight {
    /// Whether or not the address is marked unforgettable.
    unforgettable: bool,
    /// The current connection state.
    state: OutgoingStateInsight,
}

/// The state of an outgoing connection, reduced to exportable insights.
#[derive(Debug, Serialize)]
enum OutgoingStateInsight {
    Connecting {
        failures_so_far: u8,
        since: SystemTime,
    },
    Waiting {
        failures_so_far: u8,
        error: Option<String>,
        last_failure: SystemTime,
    },
    Connected {
        peer_id: NodeId,
        peer_addr: SocketAddr,
    },
    Blocked {
        since: SystemTime,
    },
    Loopback,
}

impl OutgoingStateInsight {
    /// Constructs a new outgoing state insight from a given outgoing state.
    fn from_outgoing_state<P>(
        anchor: &TimeAnchor,
        state: &OutgoingState<OutgoingHandle<P>, ConnectionError>,
    ) -> Self {
        match state {
            OutgoingState::Connecting {
                failures_so_far,
                since,
            } => OutgoingStateInsight::Connecting {
                failures_so_far: *failures_so_far,
                since: anchor.convert(*since),
            },
            OutgoingState::Waiting {
                failures_so_far,
                error,
                last_failure,
            } => OutgoingStateInsight::Waiting {
                failures_so_far: *failures_so_far,
                error: error.as_ref().map(ToString::to_string),
                last_failure: anchor.convert(*last_failure),
            },
            OutgoingState::Connected { peer_id, handle } => OutgoingStateInsight::Connected {
                peer_id: *peer_id,
                peer_addr: handle.peer_addr,
            },
            OutgoingState::Blocked { since } => OutgoingStateInsight::Blocked {
                since: anchor.convert(*since),
            },
            OutgoingState::Loopback => OutgoingStateInsight::Loopback,
        }
    }
}

/// Describes whether a connection is uni- or bi-directional.
#[derive(Debug, Serialize)]
pub(super) enum ConnectionSymmetryInsight {
    IncomingOnly {
        since: SystemTime,
        peer_addrs: BTreeSet<SocketAddr>,
    },
    OutgoingOnly {
        since: SystemTime,
    },
    Symmetric {
        peer_addrs: BTreeSet<SocketAddr>,
    },
    Gone,
}

impl ConnectionSymmetryInsight {
    /// Creates a new insight from a given connection symmetry.
    fn from_connection_symmetry(anchor: &TimeAnchor, sym: &ConnectionSymmetry) -> Self {
        match sym {
            ConnectionSymmetry::IncomingOnly { since, peer_addrs } => {
                ConnectionSymmetryInsight::IncomingOnly {
                    since: anchor.convert(*since),
                    peer_addrs: peer_addrs.clone(),
                }
            }
            ConnectionSymmetry::OutgoingOnly { since } => ConnectionSymmetryInsight::OutgoingOnly {
                since: anchor.convert(*since),
            },
            ConnectionSymmetry::Symmetric { peer_addrs } => ConnectionSymmetryInsight::Symmetric {
                peer_addrs: peer_addrs.clone(),
            },
            ConnectionSymmetry::Gone => ConnectionSymmetryInsight::Gone,
        }
    }
}

impl NetworkInsights {
    /// Collect networking insights from a given networking component.
    pub(super) fn collect_from_component<REv, P>(net: &SmallNetwork<REv, P>) -> Self
    where
        P: Payload,
    {
        // Since we are at the top level of the component, we gain access to inner values of the
        // respective structs. We abuse this to gain debugging insights. Note: If limiters are no
        // longer a `trait`, the trait methods can be removed as well in favor of direct access.
        let (priviledged_active_outgoing_nodes, priviledged_upcoming_outgoing_nodes) = net
            .outgoing_limiter
            .debug_inspect_validators()
            .map(|(a, b)| (Some(a), Some(b)))
            .unwrap_or_default();

        let anchor = TimeAnchor::now();

        let outgoing_connections = net
            .outgoing_manager
            .outgoing
            .iter()
            .map(|(addr, outgoing)| {
                let state = OutgoingStateInsight::from_outgoing_state(&anchor, &outgoing.state);
                (
                    *addr,
                    OutgoingInsight {
                        unforgettable: outgoing.is_unforgettable,
                        state,
                    },
                )
            })
            .collect();

        let connection_symmetries = net
            .connection_symmetries
            .iter()
            .map(|(id, sym)| {
                (
                    *id,
                    ConnectionSymmetryInsight::from_connection_symmetry(&anchor, sym),
                )
            })
            .collect();

        NetworkInsights {
            our_id: net.context.our_id,
            network_ca: net.context.network_ca.is_some(),
            public_addr: net.context.public_addr,
            consensus_keys: net
                .context
                .consensus_keys
                .as_ref()
                .map(ConsensusKeyPair::public_key)
                .cloned(),
            is_syncing: net.context.is_syncing.load(Ordering::Relaxed),
            net_active_era: net.active_era,
            priviledged_active_outgoing_nodes,
            priviledged_upcoming_outgoing_nodes,
            unspent_bandwidth_allowance_bytes: net
                .outgoing_limiter
                .debug_inspect_unspent_allowance(),
            outgoing_connections,
            connection_symmetries,
        }
    }
}

impl Display for NetworkInsights {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // Currently, we use the debug formatting, as it is "good enough".
        Debug::fmt(self, f)
    }
}
