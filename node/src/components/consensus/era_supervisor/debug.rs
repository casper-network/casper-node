//! Data types used solely for dumping of consensus data via the diagnostics port.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    fmt::{self, Display, Formatter},
};

use casper_types::{EraId, PublicKey, U512};
use serde::Serialize;

use crate::{
    components::consensus::{highway_core::State, ClContext, HighwayProtocol},
    types::Timestamp,
};

use super::Era;

/// Debug dump of era used for serialization.
#[derive(Debug, Serialize)]
pub(crate) struct EraDump<'a> {
    /// The era that is being dumped.
    pub(crate) id: EraId,

    /// The scheduled starting time of this era.
    pub(crate) start_time: Timestamp,
    /// The height of this era's first block.
    pub(crate) start_height: u64,

    // omitted: pending blocks
    /// Validators that have been faulty in any of the recent BONDED_ERAS switch blocks. This
    /// includes `new_faulty`.
    pub(crate) faulty: &'a HashSet<PublicKey>,
    /// Validators that are excluded from proposing new blocks.
    pub(crate) cannot_propose: &'a HashSet<PublicKey>,
    /// Accusations collected in this era so far.
    pub(crate) accusations: &'a HashSet<PublicKey>,
    /// The validator weights.
    pub(crate) validators: &'a BTreeMap<PublicKey, U512>,

    /// The state of the highway instance associated with the era.
    pub(crate) highway_state: &'a State<ClContext>,
}

impl<'a> Display for EraDump<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "era {}: TBD", self.id)
    }
}

impl<'a> EraDump<'a> {
    /// Creates a new `EraDump` from a given era.
    pub(crate) fn dump_era(era: &'a Era, era_id: EraId) -> Result<Self, Cow<'static, str>> {
        let highway = era
            .consensus
            .as_any()
            .downcast_ref::<HighwayProtocol<ClContext>>()
            .ok_or(Cow::Borrowed(
                "could not downcast `ConsensusProtocol` into `HighwayProtocol<ClContext>`",
            ))?;

        Ok(EraDump {
            id: era_id,
            start_time: era.start_time,
            start_height: era.start_height,
            faulty: &era.faulty,
            cannot_propose: &era.cannot_propose,
            accusations: &era.accusations,
            validators: &era.validators,
            highway_state: highway.highway().state(),
        })
    }
}
