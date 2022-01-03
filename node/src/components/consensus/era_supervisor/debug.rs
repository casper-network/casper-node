//! Data types used solely for dumping of consensus data via the debug console.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::{self, Display, Formatter},
};

use casper_types::{EraId, PublicKey, U512};
use serde::Serialize;

use crate::types::Timestamp;

/// Debug dump of era used for serialization.
#[derive(Debug, Serialize)]
pub(crate) struct EraDump<'a> {
    /// The era that is being dumped.
    pub(crate) id: EraId,

    /// The consensus protocol instance.
    // pub(crate) consensus: Box<dyn ConsensusProtocol<I, ClContext>>,
    /// The scheduled starting time of this era.
    pub(crate) start_time: Timestamp,
    /// The height of this era's first block.
    pub(crate) start_height: u64,

    // omitted: pending blocks
    /// Validators banned in this and the next BONDED_ERAS eras, because they were faulty in the
    /// previous switch block.
    pub(crate) new_faulty: &'a Vec<PublicKey>,
    /// Validators that have been faulty in any of the recent BONDED_ERAS switch blocks. This
    /// includes `new_faulty`.
    pub(crate) faulty: &'a HashSet<PublicKey>,
    /// Validators that are excluded from proposing new blocks.
    pub(crate) cannot_propose: &'a HashSet<PublicKey>,
    /// Accusations collected in this era so far.
    pub(crate) accusations: &'a HashSet<PublicKey>,
    /// The validator weights.
    pub(crate) validators: &'a BTreeMap<PublicKey, U512>,
}

impl<'a> Display for EraDump<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "era {}: TBD", self.id)
    }
}
