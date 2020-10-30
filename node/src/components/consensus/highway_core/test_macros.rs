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
    ($state: ident, $rng: ident, $creator: expr, $val: expr; $($obs:expr),*) => {{
        #[allow(unused_imports)] // These might be already imported at the call site.
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
        let opt_parent_hash = panorama[creator].correct();
        // Use our most recent round exponent, or the configured initial one.
        let round_exp = opt_parent_hash.map_or_else(
            || $state.params().init_round_exp(),
            |vh| $state.vote(vh).round_exp,
        );
        let value = Option::from($val);
        // At most two votes per round are allowed.
        let two_votes_limit = opt_parent_hash
            .and_then(|ph| $state.vote(ph).previous())
            .map(|pph| $state.vote(pph))
            .map(|vote| vote.round_id() + vote.round_len());
        // And our timestamp must not be less than any justification's.
        let mut timestamp = panorama
            .iter_correct(&$state)
            .map(|vote| vote.timestamp + TimeDiff::from(1))
            .chain(two_votes_limit)
            .max()
            .unwrap_or_else(Timestamp::zero);
        // If this is a block: Find the next time we're a leader.
        if value.is_some() {
            let r_len = TimeDiff::from(1 << round_exp);
            timestamp = state::round_id(timestamp + r_len - TimeDiff::from(1), round_exp);
            while $state.leader(timestamp) != creator {
                timestamp += r_len;
            }
        }
        let wvote = WireVote {
            panorama,
            creator,
            instance_id: 1u64,
            value,
            seq_number,
            timestamp,
            round_exp,
            endorsed: vec![],
        };
        let hash = wvote.hash();
        let swvote = SignedWireVote::new(wvote, &TestSecret(($creator).0), &mut $rng);
        $state.add_vote(swvote).map(|()| hash)
    }};
    ($state: ident, $rng: ident, $creator: expr, $time: expr, $round_exp: expr, $val: expr; $($obs:expr),*) => {{
        add_vote!($state, $rng, $creator, $time, $round_exp, $val; $($obs),*; vec![])
    }};
    ($state: ident, $rng: ident, $creator: expr, $time: expr, $round_exp: expr, $val: expr; $($obs:expr),*; $($ends:expr),*) => {{
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
            instance_id: 1u64,
            value: ($val).into(),
            seq_number,
            timestamp: ($time).into(),
            round_exp: $round_exp,
            endorsed: $($ends.into()),*
        };
        let hash = wvote.hash();
        let swvote = SignedWireVote::new(wvote, &TestSecret(($creator).0), &mut $rng);
        $state.add_vote(swvote).map(|()| hash)
    }};
}
