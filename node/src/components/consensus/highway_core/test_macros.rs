//! Macros for concise test setup.

/// Creates a panorama from a list of either observations or vote hashes. Vote hashes are converted
/// to `Correct` observations.
macro_rules! panorama {
    ($($obs:expr),*) => {
        crate::components::consensus::highway_core::vote::Panorama::from(vec![$($obs.into()),*])
    };
}

/// Creates a vote with the given creator, sequence number and panorama.
macro_rules! vote {
    ($creator: expr, $secret: expr, $seq_num: expr; $($obs:expr),*) => {
        vote!($creator, $secret, $seq_num; $($obs),*; None);
    };
    ($creator: expr, $secret: expr, $seq_num: expr; $($obs:expr),*; $val: expr) => {{
        let wvote = crate::components::consensus::highway_core::vertex::WireVote {
            panorama: panorama!($($obs),*),
            creator: $creator,
            value: $val,
            seq_number: $seq_num,
            timestamp: crate::types::Timestamp::zero(),
            round_exp: 12,
        };
        crate::components::consensus::highway_core::vertex::SignedWireVote::new(wvote, &$secret)
    }};
}

/// Creates a vote with the given creator, sequence number and panorama, assigns its hash to `$hash`
/// and adds the vote to `$state`. Returns an error if vote addition fails.
macro_rules! add_vote {
    ($state: ident, $hash: ident, $creator: expr, $secret: expr, $seq_num: expr; $($obs:expr),*) => {
        let vote = vote!($creator, $secret, $seq_num; $($obs),*; None);
        let $hash = vote.hash();
        $state.add_vote(vote)?;
    };
    ($state: ident, $hash: ident, $creator: expr, $secret: expr, $seq_num: expr; $($obs:expr),*; $val: expr) => {
        let vote = vote!($creator, $secret, $seq_num; $($obs),*; Some($val));
        let $hash = vote.hash();
        $state.add_vote(vote)?;
    };
}

/// Creates a vote, adds it to `$state` and returns its hash.
/// Returns an error if vote addition fails.
// TODO: Replace add_vote with this.
macro_rules! add_vote2 {
    ($state: ident, $creator: expr, $time: expr, $round_exp: expr, $val: expr; $($obs:expr),*) => {
        {
            use crate::components::consensus::highway_core::{
                state::tests::TestSecret,
                vertex::{SignedWireVote, WireVote},
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
        }
    };
}
