//! Macros for concise test setup.

/// Creates a panorama from a list of either observations or vote hashes. Vote hashes are converted
/// to `Correct` observations.
macro_rules! panorama {
    ($($obs:expr),*) => {crate::components::consensus::highway_core::vote::Panorama(vec![$($obs.into()),*])};
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
            next_round_exp: 12,
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
