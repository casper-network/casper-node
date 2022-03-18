//! Renderer for unix socket addresses.

use std::fmt::{self, Display, Formatter};

use tokio::net::unix::SocketAddr;

/// Unix socket address `Display` wrapper.
///
/// Allows displaying a unix socket address.
#[derive(Debug)]
pub(super) struct ShowUnixAddr<'a>(pub &'a SocketAddr);

impl<'a> Display for ShowUnixAddr<'a> {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.0.as_pathname() {
            Some(path) => path.display().fmt(f),
            None => f.write_str("<unnamed unix socket>"),
        }
    }
}
