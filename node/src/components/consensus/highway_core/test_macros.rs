//! Macros for concise test setup.

/// Creates a panorama from a list of either observations or vote hashes. Vote hashes are converted
/// to `Correct` observations.
macro_rules! panorama {
    ($($obs:expr),*) => {{
        use crate::components::consensus::highway_core::state::Panorama;

        Panorama::from(vec![$($obs.into()),*])
    }};
}

/// Creates a vote, adds it to `$state` and returns its hash.
/// Returns an error if vote addition fails.
///
/// The short variant is for tests that don't care about timestamps and round lengths: It
/// automatically picks reasonable values for those.
macro_rules! add_vote {
    ($state: ident, $creator: expr, $val: expr; $($obs:expr),*) => {{
        use crate::{
            components::consensus::highway_core::{
                state::{self, tests::TestSecret},
                highway::{SignedWireVote, WireVote},
            },
            types::{TimeDiff, Timestamp},
        };

        let creator = $creator;
        let panorama = panorama!($($obs),*);
        let seq_number = panorama.next_seq_num(&$state, creator);
        // Use our most recent round exponent, or the configured minimum.
        let round_exp = panorama[creator].correct().map_or_else(
            || $state.params().min_round_exp(),
            |vh| $state.vote(vh).round_exp,
        );
        let value = Option::from($val);
        // Our timestamp must not be less than any justification's.
        let min_time = panorama.iter_correct()
            .map(|vh| $state.vote(vh).timestamp + TimeDiff::from(1))
            .max()
            .unwrap_or_else(Timestamp::zero);
        let timestamp = if value.is_some() {
            // This is a block: Find the next time we're a leader.
            let mut time = state::round_id(min_time, round_exp);
            while time < min_time || $state.leader(time) != creator {
                time += TimeDiff::from(1 << round_exp);
            }
            time
        } else {
            // TODO: If we add stricter validity checks for ballots, this needs to be updated.
            min_time
        };
        let wvote = WireVote {
            panorama,
            creator,
            value,
            seq_number,
            timestamp,
            round_exp,
        };
        let hash = wvote.hash();
        let swvote = SignedWireVote::new(wvote, &TestSecret(($creator).0));
        $state.add_vote(swvote).map(|()| hash)
    }};
    ($state: ident, $creator: expr, $time: expr, $round_exp: expr, $val: expr; $($obs:expr),*) => {{
        use crate::components::consensus::highway_core::{
            state::tests::TestSecret,
            highway::{SignedWireVote, WireVote},
        };

        let creator = $creator;
        let panorama = panorama!($($obs),*);
        let seq_number = panorama.next_seq_num(&$state, creator);
        let wvote = WireVote {
            panorama,
            creator,
            value: ($val).into(),
            seq_number,
            timestamp: ($time).into(),
            round_exp: $round_exp,
        };
        let hash = wvote.hash();
        let swvote = SignedWireVote::new(wvote, &TestSecret(($creator).0));
        $state.add_vote(swvote).map(|()| hash)
    }};
}
