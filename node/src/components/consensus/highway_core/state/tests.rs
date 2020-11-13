#![allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.

use std::{
    collections::{hash_map::DefaultHasher, BTreeSet},
    hash::Hasher,
};

use rand::{Rng, RngCore};

use super::*;
use crate::{
    components::consensus::{
        highway_core::{highway::Dependency, highway_testing::TEST_BLOCK_REWARD},
        traits::ValidatorSecret,
    },
    NodeRng,
};

pub(crate) const WEIGHTS: &[Weight] = &[Weight(3), Weight(4), Weight(5)];

pub(crate) const ALICE: ValidatorIndex = ValidatorIndex(0);
pub(crate) const BOB: ValidatorIndex = ValidatorIndex(1);
pub(crate) const CAROL: ValidatorIndex = ValidatorIndex(2);
pub(crate) const DAN: ValidatorIndex = ValidatorIndex(3);
pub(crate) const ERIC: ValidatorIndex = ValidatorIndex(4);

pub(crate) const N: Observation<TestContext> = Observation::None;
pub(crate) const F: Observation<TestContext> = Observation::Faulty;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TestContext;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TestSecret(pub(crate) u32);

impl ValidatorSecret for TestSecret {
    type Hash = u64;
    type Signature = u64;

    fn sign(&self, data: &Self::Hash, _rng: &mut NodeRng) -> Self::Signature {
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

/// Returns the cause of the error, dropping the `WireUnit`.
fn unit_err(err: AddUnitError<TestContext>) -> UnitError {
    err.cause
}

/// An error that occurred when trying to add a unit.
#[derive(Debug, Error)]
#[error("{:?}", .cause)]
pub(crate) struct AddUnitError<C: Context> {
    /// The invalid unit that was not added to the protocol state.
    pub(crate) swunit: SignedWireUnit<C>,
    /// The reason the unit is invalid.
    #[source]
    pub(crate) cause: UnitError,
}

impl<C: Context> SignedWireUnit<C> {
    fn with_error(self, cause: UnitError) -> AddUnitError<C> {
        AddUnitError {
            swunit: self,
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
            19,
            4,
            u64::MAX,
            Timestamp::from(u64::MAX),
            Timestamp::from(u64::MAX),
        );
        State::new(weights, params, vec![])
    }

    /// Adds the unit to the protocol state, or returns an error if it is invalid.
    /// Panics if dependencies are not satisfied.
    pub(crate) fn add_unit(
        &mut self,
        swunit: SignedWireUnit<TestContext>,
    ) -> Result<(), AddUnitError<TestContext>> {
        if let Err(err) = self
            .pre_validate_unit(&swunit)
            .and_then(|()| self.validate_unit(&swunit))
        {
            return Err(swunit.with_error(err));
        }
        self.add_valid_unit(swunit);
        Ok(())
    }
}

#[test]
fn add_unit() -> Result<(), AddUnitError<TestContext>> {
    let mut state = State::new_test(WEIGHTS, 0);
    let mut rng = crate::new_rng();

    // Create units as follows; a0, b0 are blocks:
    //
    // Alice: a0 ————— a1
    //                /
    // Bob:   b0 —— b1
    //          \  /
    // Carol:    c0
    let a0 = add_unit!(state, rng, ALICE, 0xA; N, N, N)?;
    let b0 = add_unit!(state, rng, BOB, 48, 4u8, 0xB; N, N, N)?;
    let c0 = add_unit!(state, rng, CAROL, 49, 4u8, None; N, b0, N)?;
    let b1 = add_unit!(state, rng, BOB, 49, 4u8, None; N, b0, c0)?;
    let _a1 = add_unit!(state, rng, ALICE, None; a0, b1, c0)?;

    // Wrong sequence number: Bob hasn't produced b2 yet.
    let mut wunit = WireUnit {
        panorama: panorama!(N, b1, c0),
        creator: BOB,
        instance_id: 1u64,
        value: None,
        seq_number: 3,
        timestamp: 51.into(),
        round_exp: 4u8,
        endorsed: BTreeSet::new(),
    };
    let unit = SignedWireUnit::new(wunit.clone(), &BOB_SEC, &mut rng);
    let opt_err = state.add_unit(unit).err().map(unit_err);
    assert_eq!(Some(UnitError::SequenceNumber), opt_err);
    // Still not valid: This would be the third unit in the first round.
    wunit.seq_number = 2;
    let unit = SignedWireUnit::new(wunit, &BOB_SEC, &mut rng);
    let opt_err = state.add_unit(unit).err().map(unit_err);
    assert_eq!(Some(UnitError::ThreeUnitsInRound), opt_err);

    // Inconsistent panorama: If you see b1, you have to see c0, too.
    let opt_err = add_unit!(state, rng, CAROL, None; N, b1, N)
        .err()
        .map(unit_err);
    assert_eq!(Some(UnitError::InconsistentPanorama(BOB)), opt_err);
    // And you can't make the round exponent too small
    let opt_err = add_unit!(state, rng, CAROL, 50, 5u8, None; N, b1, c0)
        .err()
        .map(unit_err);
    assert_eq!(Some(UnitError::RoundLengthExpChangedWithinRound), opt_err);
    // And you can't make the round exponent too big
    let opt_err = add_unit!(state, rng, CAROL, 50, 40u8, None; N, b1, c0)
        .err()
        .map(unit_err);
    assert_eq!(Some(UnitError::RoundLengthExpGreaterThanMaximum), opt_err);
    // After the round from 48 to 64 has ended, the exponent can change.
    let c1 = add_unit!(state, rng, CAROL, 65, 5u8, None; N, b1, c0)?;

    // Alice has not equivocated yet, and not produced message A1.
    let missing = panorama!(F, b1, c0).missing_dependency(&state);
    assert_eq!(Some(Dependency::Evidence(ALICE)), missing);
    let missing = panorama!(42, b1, c0).missing_dependency(&state);
    assert_eq!(Some(Dependency::Unit(42)), missing);

    // Alice equivocates: A1 doesn't see a1.
    let ae1 = add_unit!(state, rng, ALICE, None; a0, b1, c0)?;
    assert!(state.has_evidence(ALICE));
    assert_eq!(panorama![F, b1, c1], *state.panorama());

    let missing = panorama!(F, b1, c0).missing_dependency(&state);
    assert_eq!(None, missing);
    let missing = panorama!(ae1, b1, c0).missing_dependency(&state);
    assert_eq!(None, missing);

    // Bob can see the equivocation.
    let b2 = add_unit!(state, rng, BOB, None; F, b1, c0)?;

    // The state's own panorama has been updated correctly.
    assert_eq!(state.panorama, panorama!(F, b2, c1));
    Ok(())
}

#[test]
fn ban_and_mark_faulty() -> Result<(), AddUnitError<TestContext>> {
    let mut rng = crate::new_rng();
    let params = Params::new(
        0,
        TEST_BLOCK_REWARD,
        TEST_BLOCK_REWARD / 5,
        4,
        19,
        4,
        u64::MAX,
        Timestamp::from(u64::MAX),
        Timestamp::from(u64::MAX),
    );
    // Everyone already knows Alice is faulty, so she is banned.
    let mut state = State::new(WEIGHTS, params, vec![ALICE]);

    assert_eq!(panorama![F, N, N], *state.panorama());
    assert_eq!(Some(&Fault::Banned), state.opt_fault(ALICE));
    let err = unit_err(add_unit!(state, rng, ALICE, 0xA; N, N, N).err().unwrap());
    assert_eq!(UnitError::Banned, err);

    state.mark_faulty(ALICE); // No change: Banned state is permanent.
    assert_eq!(panorama![F, N, N], *state.panorama());
    assert_eq!(Some(&Fault::Banned), state.opt_fault(ALICE));
    let err = unit_err(add_unit!(state, rng, ALICE, 0xA; N, N, N).err().unwrap());
    assert_eq!(UnitError::Banned, err);

    // Now we also received external evidence (i.e. not in this instance) that Bob is faulty.
    state.mark_faulty(BOB);
    assert_eq!(panorama![F, F, N], *state.panorama());
    assert_eq!(Some(&Fault::Indirect), state.opt_fault(BOB));

    // However, we still accept messages from Bob, since he is not banned.
    add_unit!(state, rng, BOB, 0xB; F, N, N)?;
    Ok(())
}

#[test]
fn find_in_swimlane() -> Result<(), AddUnitError<TestContext>> {
    let mut state = State::new_test(WEIGHTS, 0);
    let mut rng = crate::new_rng();
    let a0 = add_unit!(state, rng, ALICE, 0xA; N, N, N)?;
    let mut a = vec![a0];
    for i in 1..10 {
        let ai = add_unit!(state, rng, ALICE, None; a[i - 1], N, N)?;
        a.push(ai);
    }

    // The predecessor with sequence number i should always equal a[i].
    for j in (a.len() - 2)..a.len() {
        for i in 0..j {
            assert_eq!(Some(&a[i]), state.find_in_swimlane(&a[j], i as u64));
        }
    }

    // The skip list index of a[k] includes a[k - 2^i] for each i such that 2^i divides k.
    assert_eq!(&[a[8]], &state.unit(&a[9]).skip_idx.as_ref());
    assert_eq!(
        &[a[7], a[6], a[4], a[0]],
        &state.unit(&a[8]).skip_idx.as_ref()
    );
    Ok(())
}

#[test]
fn fork_choice() -> Result<(), AddUnitError<TestContext>> {
    let mut state = State::new_test(WEIGHTS, 0);
    let mut rng = crate::new_rng();

    // Create blocks with scores as follows:
    //
    //          a0: 7 — a1: 3
    //        /       \
    // b0: 12           b2: 4
    //        \
    //          c0: 5 — c1: 5
    let b0 = add_unit!(state, rng, BOB, 0xB0; N, N, N)?;
    let c0 = add_unit!(state, rng, CAROL, 0xC0; N, b0, N)?;
    let c1 = add_unit!(state, rng, CAROL, 0xC1; N, b0, c0)?;
    let a0 = add_unit!(state, rng, ALICE, 0xA0; N, b0, N)?;
    let b1 = add_unit!(state, rng, BOB, None; a0, b0, N)?; // Just a ballot; not shown above.
    let a1 = add_unit!(state, rng, ALICE, 0xA1; a0, b1, c1)?;
    let b2 = add_unit!(state, rng, BOB, 0xB2; a0, b1, N)?;

    // Alice built `a1` on top of `a0`, which had already 7 points.
    assert_eq!(Some(&a0), state.block(&state.unit(&a1).block).parent());
    // The fork choice is now `b2`: At height 1, `a0` wins against `c0`.
    // At height 2, `b2` wins against `a1`. `c1` has most points but is not a child of `a0`.
    assert_eq!(Some(&b2), state.fork_choice(&state.panorama));
    Ok(())
}

#[test]
fn validate_lnc_no_equivocation() -> Result<(), AddUnitError<TestContext>> {
    let mut state = State::new_test(WEIGHTS, 0);
    let mut rng = crate::new_rng();

    // No equivocations – incoming vote doesn't violate LNC.
    // Create votes as follows; a0, b0 are blocks:
    //
    // Alice: a0 — a1
    //           /
    // Bob:   b0
    let a0 = add_unit!(state, rng, ALICE, 0xA; N, N, N)?;
    let b0 = add_unit!(state, rng, BOB, 0xB; N, N, N)?;

    // a1 does not violate LNC
    let wvote = WireUnit {
        panorama: panorama!(a0, b0, N),
        creator: ALICE,
        instance_id: 1u64,
        value: None,
        seq_number: 1,
        timestamp: 51.into(),
        round_exp: 4u8,
        endorsed: BTreeSet::new(),
    };
    assert_eq!(state.validate_lnc(&wvote), None);
    Ok(())
}

#[test]
fn validate_lnc_fault_seen_directly() -> Result<(), AddUnitError<TestContext>> {
    // Equivocation cited by one honest validator in the vote's panorama.
    // Does NOT violate LNC.
    //
    // Bob:      b0
    //          / |
    // Alice: a0  |
    //            |
    //        a0' |
    //           \|
    // Carol:    c0
    let mut state = State::new_test(WEIGHTS, 0);
    let mut rng = crate::new_rng();
    let a0 = add_unit!(state, rng, ALICE, 0xA; N, N, N)?;
    let b0 = add_unit!(state, rng, BOB, 0xB; a0, N, N)?;
    let _a0_prime = add_unit!(state, rng, ALICE, 0xA2; N, N, N)?;
    // c0 does not violate LNC b/c it sees Alice as faulty.
    let c0 = WireUnit {
        panorama: panorama!(F, b0, N),
        creator: CAROL,
        instance_id: 1u64,
        value: None,
        seq_number: 0,
        timestamp: 51.into(),
        round_exp: 4u8,
        endorsed: BTreeSet::new(),
    };
    assert_eq!(state.validate_lnc(&c0), None);
    Ok(())
}

#[test]
#[ignore = "stubbed validate_lnc"]
fn validate_lnc_one_equivocator() -> Result<(), AddUnitError<TestContext>> {
    // Equivocation cited by two honest validators in the vote's panorama – their votes need to
    // be endorsed.
    //
    // Bob:      b0
    //          / \
    // Alice: a0   \
    //              \
    //        a0'    \
    //           \   |
    // Carol:    c0  |
    //             \ |
    // Dan:         d0

    let weights_dan = &[Weight(3), Weight(4), Weight(5), Weight(5)];
    let mut state = State::new_test(weights_dan, 0);
    let mut rng = crate::new_rng();
    let a0 = add_unit!(state, rng, ALICE, 0xA; N, N, N, N)?;
    let a0_prime = add_unit!(state, rng, ALICE, 0xA2; N, N, N, N)?;
    let b0 = add_unit!(state, rng, BOB, 0xB; a0, N, N, N)?;
    let c0 = add_unit!(state, rng, CAROL, 0xB2; a0_prime, N, N, N)?;
    // d0 violates LNC b/c it naively cites Alice's equivocation.
    let mut d0 = WireUnit {
        panorama: panorama!(F, b0, c0, N),
        creator: DAN,
        instance_id: 1u64,
        value: None,
        seq_number: 0,
        timestamp: 51.into(),
        round_exp: 4u8,
        endorsed: BTreeSet::new(),
    };
    // None of the votes is marked as being endorsed – violates LNC.
    assert_eq!(state.validate_lnc(&d0), Some(ALICE));
    d0.endorsed.insert(c0);
    endorse!(state, rng, CAROL, c0);
    // One endorsement isn't enough.
    assert_eq!(state.validate_lnc(&d0), Some(ALICE));
    endorse!(state, rng, BOB, c0);
    endorse!(state, rng, DAN, c0);
    // Now d0 cites non-naively b/c c0 is endorsed.
    assert_eq!(state.validate_lnc(&d0), None);
    Ok(())
}

#[test]
#[ignore = "stubbed validate_lnc"]
fn validate_lnc_two_equivocators() -> Result<(), AddUnitError<TestContext>> {
    // Multiple equivocators and indirect equivocations.
    // Votes are seen as endorsed by `state` – does not violate LNC.
    //
    // Alice   a0<---------+
    //                     |
    //         a0'<--+     |
    //               |     |
    // Bob          b0<-----------+
    //               |     |      |
    // Carol   c0<---+     |      |
    //                     |      |
    //         c0'<--------+      |
    //                     |      |
    // Dan                 d0<----+
    //                            |
    // Eric                       e0

    let weights_dan_eric = &[Weight(3), Weight(4), Weight(5), Weight(5), Weight(6)];
    let mut state = State::new_test(weights_dan_eric, 0);
    let mut rng = crate::new_rng();
    let a0 = add_unit!(state, rng, ALICE, 0xA; N, N, N, N, N)?;
    let a0_prime = add_unit!(state, rng, ALICE, 0xA2; N, N, N, N, N)?;
    let c0 = add_unit!(state, rng, CAROL, 0xC; N, N, N, N, N)?;
    let b0 = add_unit!(state, rng, BOB, 0xB; a0_prime, N, c0, N, N)?;
    let c0_prime = add_unit!(state, rng, CAROL, 0xC2; N, N, N, N, N)?;
    let d0 = add_unit!(state, rng, DAN, 0xD; a0, N, c0_prime, N, N)?;
    // e0 violates LNC b/c it naively cites Alice's & Carol's equivocations.
    let mut e0 = WireUnit {
        panorama: panorama!(F, b0, F, d0, N),
        creator: ERIC,
        instance_id: 1u64,
        value: None,
        seq_number: 0,
        timestamp: 52.into(),
        round_exp: 4u8,
        endorsed: BTreeSet::new(),
    };
    assert_eq!(state.validate_lnc(&e0), Some(ALICE));
    e0.endorsed.insert(b0);
    // Endorse b0.
    endorse!(state, rng, b0; BOB, DAN, ERIC);
    assert_eq!(state.validate_lnc(&e0), None);
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
    let mut rng = crate::new_rng();

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
