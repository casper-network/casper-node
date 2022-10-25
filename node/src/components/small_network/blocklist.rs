//! Blocklisting support.
//!
//! Blocked peers are prevent from interacting with the node through a variety of means.

use std::fmt::{self, Display, Formatter};

use serde::Serialize;

/// Reasons why a peer was blocked.
#[derive(Copy, Clone, Debug, Serialize)]
pub(crate) enum BlocklistJustification {
    /// No reason given for blocking. TODO: Remove this entirely.
    Unspecified,
}

impl Display for BlocklistJustification {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            _ => f.write_str("BlocklistJustification"),
        }
    }
}
