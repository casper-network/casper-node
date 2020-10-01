#![allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.

use std::{collections::hash_map::DefaultHasher, hash::Hasher};

use rand::{Rng, RngCore};

use super::*;
use crate::{
    components::consensus::{
        highway_core::{highway::Dependency, highway_testing::TEST_BLOCK_REWARD},
        traits::ValidatorSecret,
    },
    testing::TestRng,
    types::CryptoRngCore,
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

    fn sign(&self, data: &Self::Hash, _rng: &mut dyn CryptoRngCore) -> Self::Signature {
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
    let b0 = add_vote!(state, rng, BOB, 48, 4u8, 0xB; N, N, N)?;
    let c0 = add_vote!(state, rng, CAROL, 49, 4u8, None; N, b0, N)?;
    let b1 = add_vote!(state, rng, BOB, None; N, b0, c0)?;
    let _a1 = add_vote!(state, rng, ALICE, None; a0, b1, c0)?;

    // Wrong sequence number: Bob hasn't produced b2 yet.
    let mut wvote = WireVote {
        panorama: panorama!(N, b1, c0),
        creator: BOB,
        instance_id: 1u64,
        value: None,
        seq_number: 3,
        timestamp: 51.into(),
        round_exp: 4u8,
    };
    let vote = SignedWireVote::new(wvote.clone(), &BOB_SEC, &mut rng);
    let opt_err = state.add_vote(vote).err().map(vote_err);
    assert_eq!(Some(VoteError::SequenceNumber), opt_err);
    // Still not valid: This would be the third vote in the first round.
    wvote.seq_number = 2;
    let vote = SignedWireVote::new(wvote, &BOB_SEC, &mut rng);
    let opt_err = state.add_vote(vote).err().map(vote_err);
    assert_eq!(Some(VoteError::ThreeVotesInRound), opt_err);

    // Inconsistent panorama: If you see b1, you have to see c0, too.
    let opt_err = add_vote!(state, rng, CAROL, None; N, b1, N)
        .err()
        .map(vote_err);
    assert_eq!(Some(VoteError::InconsistentPanorama(BOB)), opt_err);
    // And you can't change the round exponent within a round.
    let opt_err = add_vote!(state, rng, CAROL, 50, 5u8, None; N, b1, c0)
        .err()
        .map(vote_err);
    assert_eq!(Some(VoteError::RoundLength), opt_err);
    // After the round from 48 to 64 has ended, the exponent can change.
    let c1 = add_vote!(state, rng, CAROL, 65, 5u8, None; N, b1, c0)?;

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
    assert_eq!(state.panorama, panorama!(F, b2, c1));
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

#[test]
fn test_leader_prng() {
    let mut rng = TestRng::new();

    // Repeat a few times to make it likely that the inner loop runs more than once.
    for _ in 0..10 {
        let upper = rng.gen_range(1, u64::MAX);
        let seed = rng.next_u64();

        // This tests that the rand crate's gen_range implementation, which is used in
        // leader_prng, doesn't change, and uses this algorithm:
        // https://github.com/rust-random/rand/blob/73befa480c58dd0461da5f4469d5e04c564d4de3/src/distributions/uniform.rs#L515
        let mut prng = ChaCha8Rng::seed_from_u64(seed);
        let zone = upper << upper.leading_zeros(); // A multiple of upper that fits into a u64.
        let expected = loop {
            // Multiply a random u64 by upper. This is between 0 and u64::MAX * upper.
            let prod = (prng.next_u64() as u128) * (upper as u128);
            // So prod >> 64 is between 0 and upper - 1. Each interval from (N << 64) to
            // (N << 64) + zone contains the same number of such values.
            // If the value is in such an interval, return N + 1; otherwise retry.
            if (prod as u64) < zone {
                break (prod >> 64) as u64 + 1;
            }
        };

        assert_eq!(expected, leader_prng(upper, seed));
    }
}

#[test]
fn test_leader_prng_values() {
    // Test a few concrete values, to detect if the ChaCha8Rng impl changes.
    assert_eq!(12578764544318200737, leader_prng(u64::MAX, 42));
    assert_eq!(12358540700710939054, leader_prng(u64::MAX, 1337));
    assert_eq!(4134160578770126600, leader_prng(u64::MAX, 0x1020304050607));
}
