//! Networking debug insights.
//!
//! The `insights` module exposes some internals of the networking component, mainly for inspection
//! through the diagnostics console. It should specifically not be used for any business logic and
//! affordances made in other corners of the `network` module to allow collecting these
//! insights should neither be abused just because they are available.

use std::{
    collections::BTreeSet,
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
    time::SystemTime,
};

use casper_types::{EraId, PublicKey};
use serde::Serialize;

use crate::{
    types::NodeId,
    utils::{opt_display::OptDisplay, DisplayIter, TimeAnchor},
};

use super::{error::ConnectionError, Network, Payload};

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
    node_key_pair: Option<PublicKey>,
    /// The active era as seen by the networking component.
    net_active_era: EraId,
}

impl NetworkInsights {
    /// Collect networking insights from a given networking component.
    pub(super) fn collect_from_component<P>(net: &Network<P>) -> Self
    where
        P: Payload,
    {
        let anchor = TimeAnchor::now();

        NetworkInsights {
            our_id: net.context.our_id(),
            network_ca: net.context.identity.network_ca.is_some(),
            public_addr: net.context.public_addr(),
            node_key_pair: net
                .context
                .node_key_pair()
                .map(|kp| kp.public_key().clone()),
            net_active_era: net.active_era,
        }
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

        Ok(())
    }
}
