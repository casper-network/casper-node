use crate::{
    components::consensus::{
        highway_core::{
            highway::SignedWireVote,
            state::{self, Panorama, State},
            validators::ValidatorIndex,
        },
        traits::Context,
    },
    types::Timestamp,
};

/// A vote sent to or received from the network.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Vote<C: Context> {
    /// The list of latest messages and faults observed by the creator of this message.
    pub(crate) panorama: Panorama<C>,
    /// The number of earlier messages by the same creator.
    pub(crate) seq_number: u64,
    /// The validator who created and sent this vote.
    pub(crate) creator: ValidatorIndex,
    /// The block this is a vote for. Either it or its parent must be the fork choice.
    pub(crate) block: C::Hash,
    /// A skip list index of the creator's swimlane, i.e. the previous vote by the same creator.
    ///
    /// For every `p = 1 << i` that divides `seq_number`, this contains an `i`-th entry pointing to
    /// the older vote with `seq_number - p`.
    pub(crate) skip_idx: Vec<C::Hash>,
    /// This vote's timestamp, in milliseconds since the epoch.
    pub(crate) timestamp: Timestamp,
    /// Original signature of the `SignedWireVote`.
    pub(crate) signature: C::Signature,
    /// The round exponent of the current round, that this message belongs to.
    ///
    /// The current round consists of all timestamps that agree with this one in all but the last
    /// `round_exp` bits.
    pub(crate) round_exp: u8,
}

impl<C: Context> Vote<C> {
    /// Creates a new `Vote` from the `WireVote`, and returns the value if it contained any.
    /// Values must be stored as a block, with the same hash.
    pub(crate) fn new(
        swvote: SignedWireVote<C>,
        fork_choice: Option<&C::Hash>,
        state: &State<C>,
    ) -> (Vote<C>, Option<C::ConsensusValue>) {
        let SignedWireVote {
            wire_vote: wvote,
            signature,
        } = swvote;
        let block = if wvote.value.is_some() {
            wvote.hash() // A vote with a new block votes for itself.
        } else {
            // If the vote didn't introduce a new block, it votes for the fork choice itself.
            // `Highway::add_vote` checks that the panorama is not empty.
            fork_choice
                .cloned()
                .expect("nonempty panorama has nonempty fork choice")
        };
        let mut skip_idx = Vec::new();
        if let Some(hash) = wvote.panorama.get(wvote.creator).correct() {
            skip_idx.push(hash.clone());
            for i in 0..wvote.seq_number.trailing_zeros() as usize {
                let old_vote = state.vote(&skip_idx[i]);
                skip_idx.push(old_vote.skip_idx[i].clone());
            }
        }
        let vote = Vote {
            panorama: wvote.panorama,
            seq_number: wvote.seq_number,
            creator: wvote.creator,
            block,
            skip_idx,
            timestamp: wvote.timestamp,
            signature,
            round_exp: wvote.round_exp,
        };
        (vote, wvote.value)
    }

    /// Returns the creator's previous message.
    pub(crate) fn previous(&self) -> Option<&C::Hash> {
        self.skip_idx.first()
    }

    /// Returns the time at which the round containing this vote began.
    pub(crate) fn round_id(&self) -> Timestamp {
        state::round_id(self.timestamp, self.round_exp)
    }
}
