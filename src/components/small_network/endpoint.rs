use std::{
    cmp::Ordering,
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

use serde::{Deserialize, Serialize};

use crate::tls::TlsCert;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub(crate) struct Endpoint {
    /// UNIX timestamp in nanoseconds resolution.
    ///
    /// Will overflow earliest November 2262.
    timestamp_ns: u64,
    /// Socket address the node is listening on.
    addr: SocketAddr,
    /// Certificate.
    cert: TlsCert,
}

impl Endpoint {
    pub(super) fn new(timestamp_ns: u64, addr: SocketAddr, cert: TlsCert) -> Self {
        Endpoint {
            timestamp_ns,
            addr,
            cert,
        }
    }

    pub(super) fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub(super) fn cert(&self) -> &TlsCert {
        &self.cert
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
