//! Macros for concise test setup.

/// Creates a panorama from a list of either observations or vote hashes. Vote hashes are converted
/// to `Correct` observations.
macro_rules! panorama {
    ($($obs:expr),*) => {crate::components::consensus::highway_core::vote::Panorama(vec![$($obs.into()),*])};
}

/// Creates a vote with the given sender, sequence number and panorama.
macro_rules! vote {
    ($sender: expr, $seq_num: expr; $($obs:expr),*) => {
        vote!($sender, $seq_num; $($obs),*; None);
    };
    ($sender: expr, $seq_num: expr; $($obs:expr),*; $val: expr) => {
        crate::components::consensus::highway_core::vertex::WireVote {
            panorama: panorama!($($obs),*),
            sender: $sender,
            values: $val,
            seq_number: $seq_num,
            instant: 0,
        }
    };
}

/// Creates a vote with the given sender, sequence number and panorama, assigns its hash to `$hash`
/// and adds the vote to `$state`. Returns an error if vote addition fails.
macro_rules! add_vote {
    ($state: ident, $hash: ident, $sender: expr, $seq_num: expr; $($obs:expr),*) => {
        let vote = vote!($sender, $seq_num; $($obs),*; None);
        let $hash = vote.hash();
        $state.add_vote(vote)?;
    };
    ($state: ident, $hash: ident, $sender: expr, $seq_num: expr; $($obs:expr),*; $val: expr) => {
        let vote = vote!($sender, $seq_num; $($obs),*; Some(vec![$val]));
        let $hash = vote.hash();
        $state.add_vote(vote)?;
    };
}
