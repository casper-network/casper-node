use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{TimeDiff, Timestamp};

use crate::components::consensus::{
    highway_core::{
        highway::SignedWireUnit,
        state::{self, Panorama, State},
    },
    traits::Context,
    utils::ValidatorIndex,
};

/// A unit sent to or received from the network.
///
/// This is only instantiated when it gets added to a `State`, and only once it has been validated.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Unit<C>
where
    C: Context,
{
    /// The list of latest units and faults observed by the creator of this message.
    /// The panorama must be valid, and this unit's creator must not be marked as faulty.
    pub panorama: Panorama<C>,
    /// The number of earlier messages by the same creator.
    /// This must be `0` if the creator's entry in the panorama is `None`. Otherwise it must be
    /// the previous unit's sequence number plus one.
    pub seq_number: u64,
    /// The validator who created and sent this unit.
    pub creator: ValidatorIndex,
    /// The block this unit votes for. Either it or its parent must be the fork choice.
    pub block: C::Hash,
    /// A skip list index of the creator's swimlane, i.e. the previous unit by the same creator.
    ///
    /// For every `p = 1 << i` that divides `seq_number`, this contains an `i`-th entry pointing to
    /// the older unit with `seq_number - p`.
    pub skip_idx: Vec<C::Hash>,
    /// This unit's timestamp, in milliseconds since the epoch. This must not be earlier than the
    /// timestamp of any unit cited in the panorama.
    pub timestamp: Timestamp,
    /// Original signature of the `SignedWireUnit`.
    pub signature: C::Signature,
    /// The length of the current round, that this message belongs to.
    ///
    /// All cited units by `creator` in the same round must have the same round length.
    pub round_len: TimeDiff,
    /// Units that this one claims are endorsed.
    /// All of these must be cited (directly or indirectly) by the panorama.
    pub endorsed: BTreeSet<C::Hash>,
}

impl<C: Context> Unit<C> {
    /// Creates a new `Unit` from the `WireUnit`, and returns the value if it contained any.
    /// Values must be stored as a block, with the same hash.
    pub(super) fn new(
        swunit: SignedWireUnit<C>,
        fork_choice: Option<&C::Hash>,
        state: &State<C>,
    ) -> (Unit<C>, Option<C::ConsensusValue>) {
        let SignedWireUnit {
            hashed_wire_unit,
            signature,
        } = swunit;
        let hash = hashed_wire_unit.hash();
        let wunit = hashed_wire_unit.into_inner();
        let block = if wunit.value.is_some() {
            hash // A unit with a new block votes for itself.
        } else {
            // If the unit didn't introduce a new block, it votes for the fork choice itself.
            // `pre_validate_unit` checks that the panorama has a `Correct` entry.
            fork_choice
                .cloned()
                .expect("nonempty panorama has nonempty fork choice")
        };
        let mut skip_idx = Vec::new();
        if let Some(hash) = wunit.panorama[wunit.creator].correct() {
            skip_idx.push(*hash);
            for i in 0..wunit.seq_number.trailing_zeros() as usize {
                let old_unit = state.unit(&skip_idx[i]);
                skip_idx.push(old_unit.skip_idx[i]);
            }
        }
        #[allow(clippy::integer_arithmetic)] // Only called with valid units.
        let round_len =
            TimeDiff::from_millis(state.params().min_round_length().millis() << wunit.round_exp);
        let unit = Unit {
            panorama: wunit.panorama,
            seq_number: wunit.seq_number,
            creator: wunit.creator,
            block,
            skip_idx,
            timestamp: wunit.timestamp,
            signature,
            round_len,
            endorsed: wunit.endorsed,
        };
        (unit, wunit.value)
    }

    /// Returns the creator's previous unit.
    pub fn previous(&self) -> Option<&C::Hash> {
        self.skip_idx.first()
    }

    /// Returns the time at which the round containing this unit began.
    pub fn round_id(&self) -> Timestamp {
        state::round_id(self.timestamp, self.round_len)
    }

    /// Returns the length of the round containing this unit.
    pub fn round_len(&self) -> TimeDiff {
        self.round_len
    }

    /// Returns whether `unit` cites a new unit from `vidx` in the last panorama.
    /// i.e. whether previous unit from creator of `vhash` cites different unit by `vidx`.
    ///
    /// NOTE: Returns `false` if `vidx` is faulty or hasn't produced any units according to the
    /// creator of `vhash`.
    pub fn new_hash_obs(&self, state: &State<C>, vidx: ValidatorIndex) -> bool {
        let latest_obs = self.panorama[vidx].correct();
        let penultimate_obs = self
            .previous()
            .and_then(|v| state.unit(v).panorama[vidx].correct());
        match (latest_obs, penultimate_obs) {
            (Some(latest_hash), Some(penultimate_hash)) => latest_hash != penultimate_hash,
            _ => false,
        }
    }

    /// Returns an iterator over units this one claims are endorsed.
    pub fn claims_endorsed(&self) -> impl Iterator<Item = &C::Hash> {
        self.endorsed.iter()
    }
}
