use std::{
    cmp::Ordering,
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

use serde::{Deserialize, Serialize};

use super::{Error, NodeId};
use crate::tls::{Signed, TlsCert};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Endpoint {
    /// UNIX timestamp in nanoseconds resolution.
    ///
    /// Will overflow earliest November 2262.
    timestamp_ns: u64,
    /// Socket address the node is listening on.
    addr: SocketAddr,
    /// Certificate.
    cert: TlsCert,
}

/// Result of an endpoint update.
///
/// Describes how an insertion of an endpoint changed an endpoint set.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(super) enum EndpointUpdate {
    /// The endpoint was previously not known.
    New { cur: Endpoint },
    /// The endpoint was known and did not change, due to being the same, more recent or invalid.
    Unchanged,
    /// The endpoint was known but with an older timestamp, only the timestamp changed.
    Refreshed { cur: Endpoint, prev: Endpoint },
    /// The endpoint changed to a different one.
    Updated { cur: Endpoint, prev: Endpoint },
    /// The signature was invalid and the endpoint discarded.
    InvalidSignature {
        signed: Signed<Endpoint>,
        err: Error,
    },
}

impl Endpoint {
    /// Creates a new endpoint.
    pub(super) fn new(timestamp_ns: u64, addr: SocketAddr, cert: TlsCert) -> Self {
        Endpoint {
            timestamp_ns,
            addr,
            cert,
        }
    }

    /// Gets the endpoint's address.
    pub(super) fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Gets the endpoint's TLS certificate.
    pub(super) fn cert(&self) -> &TlsCert {
        &self.cert
    }

    /// Get the destination of an endpoint.
    ///
    /// The destination is the endpoints socket address and certificate combined.
    pub(super) fn dest(&self) -> (SocketAddr, &TlsCert) {
        (self.addr, &self.cert)
    }

    /// Determine node ID of endpoint.
    pub(super) fn node_id(&self) -> NodeId {
        self.cert.public_key_fingerprint()
    }
}

impl Display for EndpointUpdate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EndpointUpdate::New { cur } => write!(f, "new: {}", cur),
            EndpointUpdate::Unchanged => write!(f, "unchanged"),
            EndpointUpdate::Refreshed { cur, prev } => {
                write!(f, "refreshed (cur {} prev {})", cur, prev)
            }
            EndpointUpdate::Updated { cur, prev } => write!(f, "updated: from {} to {}", prev, cur),
            EndpointUpdate::InvalidSignature { err, .. } => write!(f, "invalid signature: {}", err),
        }
    }
}

// Impose a total ordering on endpoints. Compare timestamps first, if the same, order by actual
// address. If both of these are the same, use the TLS certificate's fingerprint as a tie-breaker.
impl Ord for Endpoint {
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&self.timestamp_ns, &other.timestamp_ns)
            .then_with(|| {
                Ord::cmp(
                    &(self.addr.ip(), self.addr.port()),
                    &(other.addr.ip(), other.addr.port()),
                )
            })
            .then_with(|| Ord::cmp(&self.cert.fingerprint(), &other.cert.fingerprint()))
    }
}

impl PartialOrd for Endpoint {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for Endpoint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{} [{}]",
            self.cert.public_key_fingerprint(),
            self.addr,
            self.timestamp_ns
        )
    }
}
