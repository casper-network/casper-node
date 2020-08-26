mod block;
mod panorama;
mod params;
mod tallies;
mod vote;
mod weight;

pub(crate) use params::Params;
pub(crate) use weight::Weight;

pub(super) use panorama::{Observation, Panorama};
pub(super) use vote::Vote;

use std::{
    borrow::Borrow,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::identity,
    iter,
    ops::RangeBounds,
};

use itertools::Itertools;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use thiserror::Error;
use tracing::warn;

use crate::{
    components::consensus::{
        highway_core::{
            evidence::Evidence,
            highway::{SignedWireVote, WireVote},
            validators::{ValidatorIndex, ValidatorMap},
        },
        traits::Context,
    },
    types::{TimeDiff, Timestamp},
};
use block::Block;
use tallies::Tallies;

#[derive(Debug, Error, PartialEq)]
pub(crate) enum VoteError {
    #[error("The vote's panorama is inconsistent.")]
    Panorama,
    #[error("The vote contains the wrong sequence number.")]
    SequenceNumber,
    #[error("The vote's timestamp is older than a justification's.")]
    Timestamps,
    #[error("The creator is not a validator.")]
    Creator,
    #[error("The signature is invalid.")]
    Signature,
    #[error("The round length is invalid.")]
    RoundLength,
    #[error("The vote is a block, but its parent is already a terminal block.")]
    ValueAfterTerminalBlock,
}

/// A passive instance of the Highway protocol, containing its local state.
///
/// Both observers and active validators must instantiate this, pass in all incoming vertices from
/// peers, and use a [FinalityDetector](../finality_detector/struct.FinalityDetector.html) to
/// determine the outcome of the consensus process.
#[derive(Debug)]
pub(crate) struct State<C: Context> {
    /// The fixed parameters.
    params: Params,
    /// The validator's voting weights.
    weights: ValidatorMap<Weight>,
    /// Cumulative validator weights: Entry `i` contains the sum of the weights of validators `0`
    /// through `i`.
    cumulative_w: ValidatorMap<Weight>,
    /// All votes imported so far, by hash.
    // TODO: HashMaps prevent deterministic tests.
    votes: HashMap<C::Hash, Vote<C>>,
    /// All blocks, by hash.
    blocks: HashMap<C::Hash, Block<C>>,
    /// All block hashes, by the earliest time at which rewards for the blocks' rounds can be paid.
    reward_index: BTreeMap<Timestamp, BTreeSet<C::Hash>>,
    /// Evidence to prove a validator malicious, by index.
    evidence: HashMap<ValidatorIndex, Evidence<C>>,
    /// The full panorama, corresponding to the complete protocol state.
    panorama: Panorama<C>,
}

impl<C: Context> State<C> {
    pub(crate) fn new<I>(weights: I, params: Params) -> State<C>
    where
        I: IntoIterator,
        I::Item: Borrow<Weight>,
    {
        let weights = ValidatorMap::from(weights.into_iter().map(|w| *w.borrow()).collect_vec());
        assert!(
            weights.len() > 0,
            "cannot initialize Highway with no validators"
        );
        let mut sum = Weight(0);
        let add = |w: &Weight| {
            sum += *w;
            sum
        };
        let cumulative_w = weights.iter().map(add).collect();
        let panorama = Panorama::new(weights.len());
        State {
            params,
            weights,
            cumulative_w,
            votes: HashMap::new(),
            blocks: HashMap::new(),
            reward_index: BTreeMap::new(),
            evidence: HashMap::new(),
            panorama,
        }
    }

    /// Returns the fixed parameters.
    pub(crate) fn params(&self) -> &Params {
        &self.params
    }

    /// Returns the number of validators.
    pub(crate) fn validator_count(&self) -> usize {
        self.weights.len()
    }

    /// Returns the `idx`th validator's voting weight.
    pub(crate) fn weight(&self, idx: ValidatorIndex) -> Weight {
        self.weights[idx]
    }

    /// Returns the map of validator weights.
    pub(crate) fn weights(&self) -> &ValidatorMap<Weight> {
        &self.weights
    }

    /// Returns the total weight of all validators marked faulty in this panorama.
    pub(crate) fn faulty_weight_in(&self, panorama: &Panorama<C>) -> Weight {
        panorama
            .iter()
            .zip(&self.weights)
            .filter(|(obs, _)| **obs == Observation::Faulty)
            .map(|(_, w)| *w)
            .sum()
    }

    /// Returns the total weight of all known-faulty validators.
    pub(crate) fn faulty_weight(&self) -> Weight {
        self.faulty_weight_in(&self.panorama)
    }

    /// Returns the sum of all validators' voting weights.
    pub(crate) fn total_weight(&self) -> Weight {
        *self
            .cumulative_w
            .as_ref()
            .last()
            .expect("weight list cannot be empty")
    }

    /// Returns evidence against validator nr. `idx`, if present.
    pub(crate) fn opt_evidence(&self, idx: ValidatorIndex) -> Option<&Evidence<C>> {
        self.evidence.get(&idx)
    }

    /// Returns whether evidence against validator nr. `idx` is known.
    pub(crate) fn has_evidence(&self, idx: ValidatorIndex) -> bool {
        self.evidence.contains_key(&idx)
    }

    /// Returns an iterator over all faulty validators against which we have evidence.
    pub(crate) fn faulty_validators<'a>(&'a self) -> impl Iterator<Item = ValidatorIndex> + 'a {
        self.evidence.keys().cloned()
    }

    /// Returns the vote with the given hash, if present.
    pub(crate) fn opt_vote(&self, hash: &C::Hash) -> Option<&Vote<C>> {
        self.votes.get(hash)
    }

    /// Returns whether the vote with the given hash is known.
    pub(crate) fn has_vote(&self, hash: &C::Hash) -> bool {
        self.votes.contains_key(hash)
    }

    /// Returns the vote with the given hash. Panics if not found.
    pub(crate) fn vote(&self, hash: &C::Hash) -> &Vote<C> {
        self.opt_vote(hash).expect("vote hash must exist")
    }

    /// Returns the block contained in the vote with the given hash, if present.
    pub(crate) fn opt_block(&self, hash: &C::Hash) -> Option<&Block<C>> {
        self.blocks.get(hash)
    }

    /// Returns the block contained in the vote with the given hash. Panics if not found.
    pub(crate) fn block(&self, hash: &C::Hash) -> &Block<C> {
        self.opt_block(hash).expect("block hash must exist")
    }

    /// Returns an iterator over all hashes of blocks whose earliest timestamp for reward payout is
    /// in the specified range.
    pub(crate) fn rewards_range<RB>(&self, range: RB) -> impl Iterator<Item = &C::Hash>
    where
        RB: RangeBounds<Timestamp>,
    {
        self.reward_index
            .range(range)
            .flat_map(|(_, blocks)| blocks)
    }

    /// Returns the complete protocol state's latest panorama.
    pub(crate) fn panorama(&self) -> &Panorama<C> {
        &self.panorama
    }

    /// Returns the leader in the specified time slot.
    pub(crate) fn leader(&self, timestamp: Timestamp) -> ValidatorIndex {
        let mut rng =
            ChaCha8Rng::seed_from_u64(self.params.seed().wrapping_add(timestamp.millis()));
        // TODO: `rand` doesn't seem to document how it generates this. Needs to be portable.
        // We select a random one out of the `total_weight` weight units, starting numbering at 1.
        let r = Weight(rng.gen_range(1, self.total_weight().0 + 1));
        // The weight units are subdivided into intervals that belong to some validator.
        // `cumulative_w[i]` denotes the last weight unit that belongs to validator `i`.
        // `binary_search` returns the first `i` with `cumulative_w[i] >= r`, i.e. the validator
        // who owns the randomly selected weight unit.
        self.cumulative_w.binary_search(&r).unwrap_or_else(identity)
    }

    /// Adds the vote to the protocol state.
    ///
    /// The vote must be valid, and its dependencies satisfied.
    pub(crate) fn add_valid_vote(&mut self, swvote: SignedWireVote<C>) {
        let wvote = &swvote.wire_vote;
        self.update_panorama(&swvote);
        let hash = wvote.hash();
        let fork_choice = self.fork_choice(&wvote.panorama).cloned();
        let (vote, opt_value) = Vote::new(swvote, fork_choice.as_ref(), self);
        if let Some(value) = opt_value {
            let block = Block::new(fork_choice, value, self);
            self.reward_index
                .entry(self.reward_time(&vote))
                .or_default()
                .insert(hash.clone());
            self.blocks.insert(hash.clone(), block);
        }
        self.votes.insert(hash, vote);
    }

    pub(crate) fn add_evidence(&mut self, evidence: Evidence<C>) {
        let idx = evidence.perpetrator();
        self.evidence.insert(idx, evidence);
    }

    pub(crate) fn wire_vote(&self, hash: &C::Hash) -> Option<SignedWireVote<C>> {
        let vote = self.opt_vote(hash)?.clone();
        let opt_block = self.opt_block(hash);
        let value = opt_block.map(|block| block.value.clone());
        let wvote = WireVote {
            panorama: vote.panorama.clone(),
            creator: vote.creator,
            value,
            seq_number: vote.seq_number,
            timestamp: vote.timestamp,
            round_exp: vote.round_exp,
        };
        Some(SignedWireVote {
            wire_vote: wvote,
            signature: vote.signature,
        })
    }

    /// Returns the fork choice from `pan`'s view, or `None` if there are no blocks yet.
    ///
    /// The correct validators' latest votes count as votes for the block they point to, as well as
    /// all of its ancestors. At each level the block with the highest score is selected from the
    /// children of the previously selected block (or from all blocks at height 0), until a block
    /// is reached that has no children with any votes.
    pub(crate) fn fork_choice<'a>(&'a self, pan: &Panorama<C>) -> Option<&'a C::Hash> {
        // Collect all correct votes in a `Tallies` map, sorted by height.
        let to_entry = |(obs, w): (&Observation<C>, &Weight)| {
            let bhash = &self.vote(obs.correct()?).block;
            Some((self.block(bhash).height, bhash, *w))
        };
        let mut tallies: Tallies<C> = pan.iter().zip(&self.weights).filter_map(to_entry).collect();
        loop {
            // Find the highest block that we know is an ancestor of the fork choice.
            let (height, bhash) = tallies.find_decided(self)?;
            // Drop all votes that are not descendants of `bhash`.
            tallies = tallies.filter_descendants(height, bhash, self);
            // If there are no blocks left, `bhash` itself is the fork choice. Otherwise repeat.
            if tallies.is_empty() {
                return Some(bhash);
            }
        }
    }

    /// Returns the ancestor of the block with the given `hash`, on the specified `height`, or
    /// `None` if the block's height is lower than that.
    pub(crate) fn find_ancestor<'a>(
        &'a self,
        hash: &'a C::Hash,
        height: u64,
    ) -> Option<&'a C::Hash> {
        let block = self.block(hash);
        if block.height < height {
            return None;
        }
        if block.height == height {
            return Some(hash);
        }
        let diff = block.height - height;
        // We want to make the greatest step 2^i such that 2^i <= diff.
        let max_i = log2(diff) as usize;
        let i = max_i.min(block.skip_idx.len() - 1);
        self.find_ancestor(&block.skip_idx[i], height)
    }

    /// Returns an error if `swvote` is invalid. This can be called even if the dependencies are
    /// not present yet.
    pub(crate) fn pre_validate_vote(&self, swvote: &SignedWireVote<C>) -> Result<(), VoteError> {
        let wvote = &swvote.wire_vote;
        let creator = wvote.creator;
        if creator.0 as usize >= self.validator_count() {
            return Err(VoteError::Creator);
        }
        if wvote.round_exp < self.params.min_round_exp() {
            return Err(VoteError::RoundLength);
        }
        if (wvote.value.is_none() && !wvote.panorama.has_correct())
            || wvote.panorama.len() != self.validator_count()
            || wvote.panorama.get(creator).is_faulty()
        {
            return Err(VoteError::Panorama);
        }
        let is_terminal = |hash: &C::Hash| self.is_terminal_block(hash);
        if wvote.value.is_some() && self.fork_choice(&wvote.panorama).map_or(false, is_terminal) {
            return Err(VoteError::ValueAfterTerminalBlock);
        }
        Ok(())
    }

    /// Returns an error if `swvote` is invalid. Must only be called once all dependencies have
    /// been added to the state.
    pub(crate) fn validate_vote(&self, swvote: &SignedWireVote<C>) -> Result<(), VoteError> {
        let wvote = &swvote.wire_vote;
        let creator = wvote.creator;
        if !wvote.panorama.is_valid(self) {
            return Err(VoteError::Panorama);
        }
        let mut justifications = wvote.panorama.iter_correct();
        if !justifications.all(|vh| self.vote(vh).timestamp <= wvote.timestamp) {
            return Err(VoteError::Timestamps);
        }
        match wvote.panorama.get(creator) {
            Observation::Faulty => {
                warn!("Vote from faulty validator should be rejected in `pre_validate_vote`.");
                return Err(VoteError::Panorama);
            }
            Observation::None if wvote.seq_number == 0 => (),
            Observation::None => return Err(VoteError::SequenceNumber),
            Observation::Correct(hash) => {
                let prev_vote = self.vote(hash);
                // The sequence number must be one more than the previous vote's.
                if wvote.seq_number != 1 + prev_vote.seq_number {
                    return Err(VoteError::SequenceNumber);
                }
                // The round exponent must only change one step at a time, and not within a round.
                if prev_vote.round_exp != wvote.round_exp {
                    let max_re = prev_vote.round_exp.max(wvote.round_exp);
                    if prev_vote.round_exp + 1 < max_re
                        || wvote.round_exp + 1 < max_re
                        || prev_vote.timestamp >> max_re == wvote.timestamp >> max_re
                    {
                        return Err(VoteError::RoundLength);
                    }
                }
            }
        }
        Ok(())
    }

    /// Returns `true` if the `bhash` is a block that can have no children.
    pub(crate) fn is_terminal_block(&self, bhash: &C::Hash) -> bool {
        self.blocks.get(bhash).map_or(false, |block| {
            block.height >= self.params.end_height()
                && self.vote(bhash).timestamp >= self.params.end_timestamp()
        })
    }

    /// Updates `self.panorama` with an incoming vote. Panics if dependencies are missing.
    ///
    /// If the new vote is valid, it will just add `Observation::Correct(wvote.hash())` to the
    /// panorama. If it represents an equivocation, it adds `Observation::Faulty` and updates
    /// `self.evidence`.
    ///
    /// Panics unless all dependencies of `wvote` have already been added to `self`.
    fn update_panorama(&mut self, swvote: &SignedWireVote<C>) {
        let wvote = &swvote.wire_vote;
        let creator = wvote.creator;
        let new_obs = match (self.panorama.get(creator), wvote.panorama.get(creator)) {
            (Observation::Faulty, _) => Observation::Faulty,
            (obs0, obs1) if obs0 == obs1 => Observation::Correct(wvote.hash()),
            (Observation::None, _) => panic!("missing own previous vote"),
            (Observation::Correct(hash0), _) => {
                // If we have all dependencies of wvote and still see the sender as correct, the
                // predecessor of wvote must be a predecessor of hash0. So we already have a
                // conflicting vote with the same sequence number:
                let prev0 = self.find_in_swimlane(hash0, wvote.seq_number).unwrap();
                let wvote0 = self.wire_vote(prev0).unwrap();
                self.add_evidence(Evidence::Equivocation(wvote0, swvote.clone()));
                Observation::Faulty
            }
        };
        self.panorama[wvote.creator] = new_obs;
    }

    /// Returns the earliest time at which rewards for a block introduced by this vote can be paid.
    pub(super) fn reward_time(&self, vote: &Vote<C>) -> Timestamp {
        vote.timestamp + round_len(vote.round_exp) * self.params.reward_delay()
    }

    /// Returns the hash of the message with the given sequence number from the creator of `hash`,
    /// or `None` if the sequence number is higher than that of the vote with `hash`.
    fn find_in_swimlane<'a>(&'a self, hash: &'a C::Hash, seq_number: u64) -> Option<&'a C::Hash> {
        let vote = self.vote(hash);
        match vote.seq_number.cmp(&seq_number) {
            Ordering::Equal => Some(hash),
            Ordering::Less => None,
            Ordering::Greater => {
                let diff = vote.seq_number - seq_number;
                // We want to make the greatest step 2^i such that 2^i <= diff.
                let max_i = log2(diff) as usize;
                let i = max_i.min(vote.skip_idx.len() - 1);
                self.find_in_swimlane(&vote.skip_idx[i], seq_number)
            }
        }
    }

    /// Returns an iterator over votes (with hashes) by the same creator, in reverse chronological
    /// order, starting with the specified vote.
    pub(crate) fn swimlane<'a>(
        &'a self,
        vhash: &'a C::Hash,
    ) -> impl Iterator<Item = (&'a C::Hash, &'a Vote<C>)> {
        let mut next = Some(vhash);
        iter::from_fn(move || {
            let current = next?;
            let vote = self.vote(current);
            next = vote.previous();
            Some((current, vote))
        })
    }

    /// Returns a vector of validator indexes that equivocated between block
    /// identified by `fhash` and its parent.
    pub(super) fn get_new_equivocators(&self, fhash: &C::Hash) -> Vec<ValidatorIndex> {
        let cvote = self.vote(fhash);
        let mut equivocators: Vec<ValidatorIndex> = Vec::new();
        let fblock = self.block(fhash);
        let empty_panorama = Panorama::new(self.validator_count());
        let pvpanorama = fblock
            .parent()
            .map(|pvhash| &self.vote(pvhash).panorama)
            .unwrap_or(&empty_panorama);
        for (vid, obs) in cvote.panorama.enumerate() {
            // If validator is faulty in candidate's panorama but not in its
            // parent, it means it's a "new" equivocator.
            if obs.is_faulty() && !pvpanorama.get(vid).is_faulty() {
                equivocators.push(vid)
            }
        }
        equivocators
    }
}

/// Returns the round length, given the round exponent.
pub(super) fn round_len(round_exp: u8) -> TimeDiff {
    TimeDiff::from(1 << round_exp)
}

/// Returns the time at which the round with the given timestamp and round exponent began.
///
/// The boundaries of rounds with length `1 << round_exp` are multiples of that length, in
/// milliseconds since the epoch. So the beginning of the current round is the greatest multiple
/// of `1 << round_exp` that is less or equal to `timestamp`.
pub(super) fn round_id(timestamp: Timestamp, round_exp: u8) -> Timestamp {
    // The greatest multiple less or equal to the timestamp is the timestamp with the last
    // `round_exp` bits set to zero.
    (timestamp >> round_exp) << round_exp
}

/// Returns the base-2 logarithm of `x`, rounded down,
/// i.e. the greatest `i` such that `2.pow(i) <= x`.
fn log2(x: u64) -> u32 {
    // The least power of two that is strictly greater than x.
    let next_pow2 = (x + 1).next_power_of_two();
    // It's twice as big as the greatest power of two that is less or equal than x.
    let prev_pow2 = next_pow2 >> 1;
    // The number of trailing zeros is its base-2 logarithm.
    prev_pow2.trailing_zeros()
}

#[allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.
#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::hash_map::DefaultHasher, hash::Hasher};

    use rand::{CryptoRng, Rng};

    use super::*;
    use crate::{
        components::consensus::{
            highway_core::{
                highway::Dependency,
                highway_testing::{TEST_BLOCK_REWARD, TEST_REWARD_DELAY},
            },
            traits::ValidatorSecret,
        },
        testing::TestRng,
    };

    pub(crate) const WEIGHTS: &[Weight] = &[Weight(3), Weight(4), Weight(5)];

    pub(crate) const ALICE: ValidatorIndex = ValidatorIndex(0);
    pub(crate) const BOB: ValidatorIndex = ValidatorIndex(1);
    pub(crate) const CAROL: ValidatorIndex = ValidatorIndex(2);

    pub(crate) const N: Observation<TestContext> = Observation::None;
    pub(crate) const F: Observation<TestContext> = Observation::Faulty;

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub(crate) struct TestContext;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct TestSecret(pub(crate) u32);

    impl ValidatorSecret for TestSecret {
        type Hash = u64;
        type Signature = u64;

        fn sign<R: Rng + CryptoRng + ?Sized>(
            &self,
            data: &Self::Hash,
            _rng: &mut R,
        ) -> Self::Signature {
            data + u64::from(self.0)
        }
    }

    pub(crate) const ALICE_SEC: TestSecret = TestSecret(0);
    pub(crate) const BOB_SEC: TestSecret = TestSecret(1);
    pub(crate) const CAROL_SEC: TestSecret = TestSecret(2);

    impl Context for TestContext {
        type ConsensusValue = u32;
        type ValidatorId = u32;
        type ValidatorSecret = TestSecret;
        type Signature = u64;
        type Hash = u64;
        type InstanceId = u64;

        fn hash(data: &[u8]) -> Self::Hash {
            let mut hasher = DefaultHasher::new();
            hasher.write(data);
            hasher.finish()
        }

        fn verify_signature(
            hash: &Self::Hash,
            public_key: &Self::ValidatorId,
            signature: &<Self::ValidatorSecret as ValidatorSecret>::Signature,
        ) -> bool {
            let computed_signature = hash + u64::from(*public_key);
            computed_signature == *signature
        }
    }

    impl From<<TestContext as Context>::Hash> for Observation<TestContext> {
        fn from(vhash: <TestContext as Context>::Hash) -> Self {
            Observation::Correct(vhash)
        }
    }

    /// Returns the cause of the error, dropping the `WireVote`.
    fn vote_err(err: AddVoteError<TestContext>) -> VoteError {
        err.cause
    }

    /// An error that occurred when trying to add a vote.
    #[derive(Debug, Error)]
    #[error("{:?}", .cause)]
    pub(crate) struct AddVoteError<C: Context> {
        /// The invalid vote that was not added to the protocol state.
        pub(crate) swvote: SignedWireVote<C>,
        /// The reason the vote is invalid.
        #[source]
        pub(crate) cause: VoteError,
    }

    impl<C: Context> SignedWireVote<C> {
        fn with_error(self, cause: VoteError) -> AddVoteError<C> {
            AddVoteError {
                swvote: self,
                cause,
            }
        }
    }

    impl State<TestContext> {
        /// Returns a new `State` with `TestContext` parameters suitable for tests.
        pub(crate) fn new_test(weights: &[Weight], seed: u64) -> Self {
            let params = Params::new(
                seed,
                TEST_BLOCK_REWARD,
                TEST_BLOCK_REWARD / 5,
                TEST_REWARD_DELAY,
                4,
                u64::MAX,
                Timestamp::from(u64::MAX),
            );
            State::new(weights, params)
        }

        /// Adds the vote to the protocol state, or returns an error if it is invalid.
        /// Panics if dependencies are not satisfied.
        pub(crate) fn add_vote(
            &mut self,
            swvote: SignedWireVote<TestContext>,
        ) -> Result<(), AddVoteError<TestContext>> {
            if let Err(err) = self.validate_vote(&swvote) {
                return Err(swvote.with_error(err));
            }
            self.add_valid_vote(swvote);
            Ok(())
        }
    }

    #[test]
    fn add_vote() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(WEIGHTS, 0);
        let mut rng = TestRng::new();

        // Create votes as follows; a0, b0 are blocks:
        //
        // Alice: a0 ————— a1
        //                /
        // Bob:   b0 —— b1
        //          \  /
        // Carol:    c0
        let a0 = add_vote!(state, rng, ALICE, 0xA; N, N, N)?;
        let b0 = add_vote!(state, rng, BOB, 0xB; N, N, N)?;
        let c0 = add_vote!(state, rng, CAROL, None; N, b0, N)?;
        let b1 = add_vote!(state, rng, BOB, None; N, b0, c0)?;
        let _a1 = add_vote!(state, rng, ALICE, None; a0, b1, c0)?;

        // Wrong sequence number: Carol hasn't produced c1 yet.
        let wvote = WireVote {
            panorama: panorama!(N, b1, c0),
            creator: CAROL,
            value: None,
            seq_number: 2,
            timestamp: state.vote(&b1).timestamp + TimeDiff::from(1),
            round_exp: state.vote(&c0).round_exp,
        };
        let vote = SignedWireVote::new(wvote, &CAROL_SEC, &mut rng);
        let opt_err = state.add_vote(vote).err().map(vote_err);
        assert_eq!(Some(VoteError::SequenceNumber), opt_err);
        // Inconsistent panorama: If you see b1, you have to see c0, too.
        let opt_err = add_vote!(state, rng, CAROL, None; N, b1, N)
            .err()
            .map(vote_err);
        assert_eq!(Some(VoteError::Panorama), opt_err);

        // Alice has not equivocated yet, and not produced message A1.
        let missing = panorama!(F, b1, c0).missing_dependency(&state);
        assert_eq!(Some(Dependency::Evidence(ALICE)), missing);
        let missing = panorama!(42, b1, c0).missing_dependency(&state);
        assert_eq!(Some(Dependency::Vote(42)), missing);

        // Alice equivocates: A1 doesn't see a1.
        let ae1 = add_vote!(state, rng, ALICE, None; a0, b1, c0)?;
        assert!(state.has_evidence(ALICE));

        let missing = panorama!(F, b1, c0).missing_dependency(&state);
        assert_eq!(None, missing);
        let missing = panorama!(ae1, b1, c0).missing_dependency(&state);
        assert_eq!(None, missing);

        // Bob can see the equivocation.
        let b2 = add_vote!(state, rng, BOB, None; F, b1, c0)?;

        // The state's own panorama has been updated correctly.
        assert_eq!(state.panorama, panorama!(F, b2, c0));
        Ok(())
    }

    #[test]
    fn find_in_swimlane() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(WEIGHTS, 0);
        let mut rng = TestRng::new();
        let a0 = add_vote!(state, rng, ALICE, 0xA; N, N, N)?;
        let mut a = vec![a0];
        for i in 1..10 {
            let ai = add_vote!(state, rng, ALICE, None; a[i - 1], N, N)?;
            a.push(ai);
        }

        // The predecessor with sequence number i should always equal a[i].
        for j in (a.len() - 2)..a.len() {
            for i in 0..j {
                assert_eq!(Some(&a[i]), state.find_in_swimlane(&a[j], i as u64));
            }
        }

        // The skip list index of a[k] includes a[k - 2^i] for each i such that 2^i divides k.
        assert_eq!(&[a[8]], &state.vote(&a[9]).skip_idx.as_ref());
        assert_eq!(
            &[a[7], a[6], a[4], a[0]],
            &state.vote(&a[8]).skip_idx.as_ref()
        );
        Ok(())
    }

    #[test]
    fn fork_choice() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(WEIGHTS, 0);
        let mut rng = TestRng::new();

        // Create blocks with scores as follows:
        //
        //          a0: 7 — a1: 3
        //        /       \
        // b0: 12           b2: 4
        //        \
        //          c0: 5 — c1: 5
        let b0 = add_vote!(state, rng, BOB, 0xB0; N, N, N)?;
        let c0 = add_vote!(state, rng, CAROL, 0xC0; N, b0, N)?;
        let c1 = add_vote!(state, rng, CAROL, 0xC1; N, b0, c0)?;
        let a0 = add_vote!(state, rng, ALICE, 0xA0; N, b0, N)?;
        let b1 = add_vote!(state, rng, BOB, None; a0, b0, N)?; // Just a ballot; not shown above.
        let a1 = add_vote!(state, rng, ALICE, 0xA1; a0, b1, c1)?;
        let b2 = add_vote!(state, rng, BOB, 0xB2; a0, b1, N)?;

        // Alice built `a1` on top of `a0`, which had already 7 points.
        assert_eq!(Some(&a0), state.block(&state.vote(&a1).block).parent());
        // The fork choice is now `b2`: At height 1, `a0` wins against `c0`.
        // At height 2, `b2` wins against `a1`. `c1` has most points but is not a child of `a0`.
        assert_eq!(Some(&b2), state.fork_choice(&state.panorama));
        Ok(())
    }

    #[test]
    fn test_log2() {
        assert_eq!(2, log2(0b100));
        assert_eq!(2, log2(0b101));
        assert_eq!(2, log2(0b111));
        assert_eq!(3, log2(0b1000));
    }
}
