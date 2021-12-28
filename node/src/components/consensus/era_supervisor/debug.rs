//! Data types used solely for dumping of consensus data via the debug console.

use std::fmt::{self, Display, Formatter};

use casper_types::EraId;
use serde::Serialize;

/// Debug dump of era used for serialization.
#[derive(Debug, Serialize)]
pub(crate) struct EraDump<'a> {
    /// The era that is being dumped.
    pub(crate) id: EraId,
    /// The actual era data.
    pub(crate) data: &'a (),
}

impl<'a> Display for EraDump<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "era {}: TBD", self.id)
    }
}
