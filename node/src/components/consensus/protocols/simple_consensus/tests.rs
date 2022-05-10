use super::*;

use std::{collections::BTreeSet, sync::Arc};

use casper_types::{PublicKey, SecretKey, Timestamp, U512};

use crate::{
    components::consensus::{
        cl_context::{ClContext, Keypair},
        config::Config,
        consensus_protocol::{ConsensusProtocol, ProtocolOutcome},
        leader_sequence,
        protocols::common,
        tests::utils::{
            new_test_chainspec, ALICE_NODE_ID, ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_PUBLIC_KEY,
            BOB_SECRET_KEY, CAROL_PUBLIC_KEY, CAROL_SECRET_KEY,
        },
        traits::Context,
    },
    types::BlockPayload,
};

const INSTANCE_ID_DATA: &[u8; 1] = &[123u8; 1];

/// Creates a new `SimpleConsensus` instance.
///
/// The random seed is selected so that the leader sequence starts with `seq`.
pub(crate) fn new_test_simple_consensus<I1, I2, T>(
    weights: I1,
    init_faulty: I2,
    seq: &[ValidatorIndex],
) -> SimpleConsensus<ClContext>
where
    I1: IntoIterator<Item = (PublicKey, T)>,
    I2: IntoIterator<Item = PublicKey>,
    T: Into<U512>,
{
    let weights = weights
        .into_iter()
        .map(|(pk, w)| (pk, w.into()))
        .collect::<Vec<_>>();
    let mut chainspec = new_test_chainspec(weights.clone());
    chainspec.core_config.minimum_era_height = 3;
    let mut config = Config::default();
    config.simple_consensus.standstill_timeout = Some("1sec".parse().unwrap());
    let validators = common::validators::<ClContext>(
        &Default::default(),
        &Default::default(),
        weights.iter().cloned().collect(),
    );
    let weights_vmap = common::validator_weights::<ClContext>(&validators);
    let leaders = weights.iter().map(|_| true).collect();
    let seed = leader_sequence::find_seed(seq, &weights_vmap, &leaders);
    // Timestamp of the genesis era start and test start.
    let start_timestamp: Timestamp = 0.into();
    SimpleConsensus::<ClContext>::new(
        ClContext::hash(INSTANCE_ID_DATA),
        weights.into_iter().collect(),
        &init_faulty.into_iter().collect(),
        &None.into_iter().collect(),
        &chainspec,
        &config,
        None,
        start_timestamp,
        seed,
        start_timestamp,
    )
}

/// Creates a serialized `Message::Signed`.
fn create_message(
    validators: &Validators<PublicKey>,
    round_id: RoundId,
    content: Content<ClContext>,
    keypair: &Keypair,
) -> Vec<u8> {
    let validator_idx = validators.get_index(keypair.public_key()).unwrap();
    let instance_id = ClContext::hash(INSTANCE_ID_DATA);
    let serialized_fields = bincode::serialize(&(round_id, &instance_id, &content, validator_idx))
        .expect("failed to serialize fields");
    let hash = ClContext::hash(&serialized_fields);
    let signature = keypair.sign(&hash);
    Message::Signed {
        round_id,
        instance_id,
        content,
        validator_idx,
        signature,
    }
    .serialize()
}

/// Removes all `CreatedGossipMessage`s from `outcomes` and returns the deserialized content of all
/// `Message::Signed`, after verifying the signatures.
fn remove_gossip(
    validators: &Validators<PublicKey>,
    outcomes: &mut ProtocolOutcomes<ClContext>,
) -> HashSet<(RoundId, PublicKey, Content<ClContext>)> {
    let mut result = HashSet::new();
    let expected_instance_id = ClContext::hash(INSTANCE_ID_DATA);
    outcomes.retain(|outcome| {
        let msg = match outcome {
            ProtocolOutcome::CreatedGossipMessage(msg) => msg,
            _ => return true,
        };
        if let Message::Signed {
            round_id,
            instance_id,
            content,
            validator_idx,
            signature,
        } =
            bincode::deserialize::<Message<ClContext>>(msg.as_slice()).expect("deserialize message")
        {
            assert_eq!(instance_id, expected_instance_id);
            let serialized_fields =
                bincode::serialize(&(round_id, &instance_id, &content, validator_idx))
                    .expect("failed to serialize fields");
            let hash = ClContext::hash(&serialized_fields);
            let public_key = validators.id(validator_idx).expect("validator ID").clone();
            assert!(ClContext::verify_signature(&hash, &public_key, &signature));
            assert!(result.insert((round_id, public_key, content)));
            false
        } else {
            true
        }
    });
    result
}

/// Removes all `CreatedMessageToRandomPeer`s from `outcomes` and returns the deserialized messages.
fn remove_messages_to_random(
    outcomes: &mut ProtocolOutcomes<ClContext>,
) -> Vec<Message<ClContext>> {
    let mut result = Vec::new();
    let expected_instance_id = ClContext::hash(INSTANCE_ID_DATA);
    outcomes.retain(|outcome| {
        let msg = match outcome {
            ProtocolOutcome::CreatedMessageToRandomPeer(msg) => {
                bincode::deserialize::<Message<ClContext>>(msg.as_slice())
                    .expect("deserialize message")
            }
            _ => return true,
        };
        if let Message::SyncState { instance_id, .. } = &msg {
            assert_eq!(*instance_id, expected_instance_id);
            result.push(msg);
            false
        } else {
            panic!("unexpected message to random peer: {:?}", msg);
        }
    });
    result
}

/// Removes all `CreatedTargetedMessage`s from `outcomes` and returns the deserialized content of
/// all `Message::Signed`, after verifying the signatures.
fn remove_targeted_messages(
    validators: &Validators<PublicKey>,
    outcomes: &mut ProtocolOutcomes<ClContext>,
) -> HashSet<(NodeId, RoundId, PublicKey, Content<ClContext>)> {
    let mut result = HashSet::new();
    let expected_instance_id = ClContext::hash(INSTANCE_ID_DATA);
    outcomes.retain(|outcome| {
        let (msg, peer) = match outcome {
            ProtocolOutcome::CreatedTargetedMessage(msg, peer) => (msg, *peer),
            _ => return true,
        };
        if let Message::Signed {
            round_id,
            instance_id,
            content,
            validator_idx,
            signature,
        } =
            bincode::deserialize::<Message<ClContext>>(msg.as_slice()).expect("deserialize message")
        {
            assert_eq!(instance_id, expected_instance_id);
            let serialized_fields =
                bincode::serialize(&(round_id, &instance_id, &content, validator_idx))
                    .expect("failed to serialize fields");
            let hash = ClContext::hash(&serialized_fields);
            let public_key = validators.id(validator_idx).expect("validator ID").clone();
            assert!(ClContext::verify_signature(&hash, &public_key, &signature));
            assert!(result.insert((peer, round_id, public_key, content)));
            false
        } else {
            true
        }
    });
    result
}

/// Expects exactly one `CreateNewBlock` in `outcomes`, removes and returns it.
fn remove_create_new_block(outcomes: &mut ProtocolOutcomes<ClContext>) -> BlockContext<ClContext> {
    let mut result = None;
    outcomes.retain(|outcome| match outcome {
        ProtocolOutcome::CreateNewBlock(block_context) => {
            if let Some(other_context) = result.replace(block_context.clone()) {
                panic!(
                    "got multiple CreateNewBlock outcomes: {:?}, {:?}",
                    other_context, block_context
                );
            }
            false
        }
        _ => true,
    });
    result.expect("missing CreateNewBlock outcome")
}

/// Checks that the `proposals` match the `FinalizedBlock` outcomes.
fn expect_finalized(
    outcomes: &ProtocolOutcomes<ClContext>,
    proposals: &[(&Proposal<ClContext>, u64)],
) {
    let mut proposals_iter = proposals.iter();
    for outcome in outcomes {
        if let ProtocolOutcome::FinalizedBlock(fb) = outcome {
            if let Some(&(proposal, rel_height)) = proposals_iter.next() {
                assert_eq!(fb.relative_height, rel_height);
                assert_eq!(fb.timestamp, proposal.timestamp);
                assert_eq!(Some(&fb.value), proposal.maybe_block.as_ref());
            } else {
                panic!("unexpected finalized block {:?}", fb);
            }
        }
    }
    assert_eq!(None, proposals_iter.next(), "missing finalized proposal");
}

/// Checks that `outcomes` contains no `FinalizedBlock`, `CreateNewBlock` or `CreatedGossipMessage`.
fn expect_no_gossip_block_finalized(outcomes: ProtocolOutcomes<ClContext>) {
    for outcome in outcomes {
        match outcome {
            ProtocolOutcome::FinalizedBlock(fb) => panic!("unexpected finalized block: {:?}", fb),
            ProtocolOutcome::CreatedGossipMessage(msg) => {
                let deserialized = bincode::deserialize::<Message<ClContext>>(msg.as_slice())
                    .expect("deserialize message");
                panic!("unexpected gossip message {:?}", deserialized);
            }
            ProtocolOutcome::CreateNewBlock(block_context) => {
                panic!("unexpected CreateNewBlock: {:?}", block_context);
            }
            _ => {}
        }
    }
}

/// Checks that the expected timer was requested by the protocol.
fn expect_timer(outcomes: &ProtocolOutcomes<ClContext>, timestamp: Timestamp, timer_id: TimerId) {
    assert!(
        outcomes.contains(&ProtocolOutcome::ScheduleTimer(timestamp, timer_id)),
        "missing timer {} for {:?} from {:?}",
        timer_id.0,
        timestamp,
        outcomes
    );
}

/// Creates a new payload with the given random bit and no deploys or transfers.
fn new_payload(random_bit: bool) -> Arc<BlockPayload> {
    Arc::new(BlockPayload::new(vec![], vec![], vec![], random_bit))
}

fn proposal(p: &Proposal<ClContext>) -> Content<ClContext> {
    Content::Proposal(p.clone())
}

fn vote(v: bool) -> Content<ClContext> {
    Content::Vote(v)
}

fn echo(hash: <ClContext as Context>::Hash) -> Content<ClContext> {
    Content::Echo(hash)
}

fn abc_weights(
    alice_w: u64,
    bob_w: u64,
    carol_w: u64,
) -> (Vec<(PublicKey, U512)>, Validators<PublicKey>) {
    let weights: Vec<(PublicKey, U512)> = vec![
        (ALICE_PUBLIC_KEY.clone(), U512::from(alice_w)),
        (BOB_PUBLIC_KEY.clone(), U512::from(bob_w)),
        (CAROL_PUBLIC_KEY.clone(), U512::from(carol_w)),
    ];
    let validators = common::validators::<ClContext>(
        &Default::default(),
        &Default::default(),
        weights.iter().cloned().collect(),
    );
    (weights, validators)
}

/// Tests the core logic of the consensus protocol, i.e. the criteria for sending votes and echoes
/// and finalizing blocks.
///
/// In this scenario Alice has 60%, Bob 30% and Carol 10% of the weight, and we create Carol's
/// consensus instance. Bob makes a proposal in round 0. Alice doesn't see it and makes a proposal
/// without a parent (skipping round 0) in round 1, and proposes a child of that one in round 2.
///
/// The fork is resolved in Alice's favor: Round 0 becomes skippable and round 2 committed, so
/// Alice's two blocks become finalized.
#[test]
fn simple_consensus_no_fault() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(60, 30, 10);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leaders are Bob, Alice, Alice, Carol, Carol.
    let leader_seq = &[bob_idx, alice_idx, alice_idx, carol_idx, carol_idx];
    let mut sc_c = new_test_simple_consensus(weights, vec![], leader_seq);

    let alice_kp = Keypair::from(ALICE_SECRET_KEY.clone());
    let bob_kp = Keypair::from(BOB_SECRET_KEY.clone());
    let carol_kp = Keypair::from(CAROL_SECRET_KEY.clone());

    sc_c.activate_validator(CAROL_PUBLIC_KEY.clone(), carol_kp, Timestamp::now(), None);

    let block_time = sc_c.params.min_block_time();

    let sender = *ALICE_NODE_ID;
    let mut timestamp = Timestamp::from(100000);

    let proposal0 = Proposal {
        timestamp,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash0 = proposal0.hash();

    let proposal1 = Proposal {
        timestamp,
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash1 = proposal1.hash();

    let proposal2 = Proposal {
        timestamp: timestamp + block_time,
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: Some(1),
        inactive: Some(Default::default()),
    };
    let hash2 = proposal2.hash();

    let proposal3 = Proposal {
        timestamp: timestamp + block_time * 2,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: Some(2),
        inactive: Some(Default::default()),
    };
    let hash3 = proposal3.hash();

    let proposal4 = Proposal::<ClContext> {
        timestamp: timestamp + block_time * 3,
        maybe_block: None,
        maybe_parent_round_id: Some(3),
        inactive: None,
    };
    let hash4 = proposal4.hash();

    // Carol's node joins a bit late, and gets some messages out of order.
    timestamp += block_time;

    // Alice makes a proposal in round 2 with parent in round 1. Alice and Bob echo it.
    let msg = create_message(&validators, 2, proposal(&proposal2), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, echo(hash2), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, echo(hash2), &bob_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // Alice and Bob even vote for it, so the round is committed!
    // But without an accepted parent it isn't finalized yet.
    let msg = create_message(&validators, 2, vote(true), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, vote(true), &bob_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // Alice makes a proposal in round 1 with no parent, and echoes it.
    let msg = create_message(&validators, 1, proposal(&proposal1), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 1, echo(hash1), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // Now Carol receives Bob's proposal in round 0. Carol echoes it.
    let msg = create_message(&validators, 0, proposal(&proposal0), &bob_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.remove(&(0, CAROL_PUBLIC_KEY.clone(), echo(hash0))));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    expect_no_gossip_block_finalized(outcomes);

    // Alice also echoes Bob's round 0 proposal, so it has a quorum and is accepted.
    // With that round 1 becomes current and Carol echoes Alice's proposal. That makes a quorum, but
    // since round 0 is not skippable round 1 is not yet accepted and thus round 2 is not yet
    // current.
    let msg = create_message(&validators, 0, echo(hash0), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.remove(&(1, CAROL_PUBLIC_KEY.clone(), echo(hash1))));
    assert!(gossip.remove(&(0, CAROL_PUBLIC_KEY.clone(), vote(true))));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);

    // Bob votes false in round 0. That's not a quorum yet.
    let msg = create_message(&validators, 0, vote(false), &bob_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // But with Alice's vote round 0 becomes skippable. That means rounds 1 and 2 are now accepted
    // and Carol votes for them. Since round 2 is already committed, both 1 and 2 are finalized.
    // Since round 2 became current, Carol echoes the proposal, too.
    let msg = create_message(&validators, 0, vote(false), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.remove(&(2, CAROL_PUBLIC_KEY.clone(), echo(hash2))));
    assert!(gossip.remove(&(1, CAROL_PUBLIC_KEY.clone(), vote(true))));
    assert!(gossip.remove(&(2, CAROL_PUBLIC_KEY.clone(), vote(true))));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    expect_finalized(&outcomes, &[(&proposal1, 0), (&proposal2, 1)]);
    expect_timer(&outcomes, timestamp + block_time, TIMER_ID_UPDATE);

    timestamp += block_time;

    // In round 3 Carol is the leader, so she creates a new block to propose.
    let mut outcomes = sc_c.handle_timer(timestamp, TIMER_ID_UPDATE, &mut rng);
    let block_context = remove_create_new_block(&mut outcomes);
    expect_no_gossip_block_finalized(outcomes);
    assert_eq!(block_context.timestamp(), timestamp);
    assert_eq!(block_context.ancestor_values().len(), 2);

    let proposed_block = ProposedBlock::new(new_payload(false), block_context);
    let mut outcomes = sc_c.propose(proposed_block, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.remove(&(3, CAROL_PUBLIC_KEY.clone(), proposal(&proposal3))));
    assert!(gossip.remove(&(3, CAROL_PUBLIC_KEY.clone(), echo(hash3))));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);

    timestamp += block_time;

    // Once Alice echoes Carol's proposal, she can go on to propose in round 4, too.
    // Since the round height is 3, the 4th proposal does not contain a block.
    let msg = create_message(&validators, 3, echo(hash3), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.remove(&(3, CAROL_PUBLIC_KEY.clone(), vote(true))));
    assert!(gossip.remove(&(4, CAROL_PUBLIC_KEY.clone(), proposal(&proposal4))));
    assert!(gossip.remove(&(4, CAROL_PUBLIC_KEY.clone(), echo(hash4))));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);

    // Only when Alice also votes for the switch block is it finalized.
    assert!(!sc_c.finalized_switch_block());
    let msg = create_message(&validators, 3, vote(true), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    expect_finalized(&outcomes, &[(&proposal3, 2)]);
    assert!(sc_c.finalized_switch_block());
}

/// Tests that a faulty validator counts towards every quorum.
///
/// In this scenario Alice has 60% of the weight, Bob 10% and Carol 30%. Carol is offline and Bob is
/// faulty. Alice proposes a few blocks but can't finalize them alone. Once Bob double-signs, he
/// counts towards every quorum and Alice's messages suffice to finalize her blocks.
#[test]
fn simple_consensus_faults() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(60, 10, 30);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leaders are Carol, Alice, Alice.
    let mut sc = new_test_simple_consensus(weights, vec![], &[carol_idx, alice_idx, alice_idx]);

    let alice_kp = Keypair::from(ALICE_SECRET_KEY.clone());
    let bob_kp = Keypair::from(BOB_SECRET_KEY.clone());
    let carol_kp = Keypair::from(CAROL_SECRET_KEY.clone());

    let sender = *ALICE_NODE_ID;
    let mut timestamp = Timestamp::now();

    let proposal1 = Proposal {
        timestamp,
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash1 = proposal1.hash();

    let proposal2 = Proposal {
        timestamp: timestamp + sc.params.min_block_time(),
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: Some(1),
        inactive: Some(iter::once(carol_idx).collect()),
    };
    let hash2 = proposal2.hash();

    timestamp += sc.params.min_block_time();

    // Alice makes sproposals in rounds 1 and 2, echoes and votes for them.
    let msg = create_message(&validators, 1, proposal(&proposal1), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 1, echo(hash1), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 1, vote(true), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, proposal(&proposal2), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, echo(hash2), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, vote(true), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));

    // Since Carol did not make a proposal Alice votes to make round 0 skippable.
    let msg = create_message(&validators, 0, vote(false), &alice_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));

    // Carol is offline and Alice alone does not have a quorum.
    // But if Bob equivocates, he counts towards every quorum, so the blocks get finalized.
    let msg = create_message(&validators, 3, vote(true), &bob_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 3, vote(false), &bob_kp);
    let outcomes = sc.handle_message(&mut rng, sender, msg, timestamp);
    expect_finalized(&outcomes, &[(&proposal1, 0), (&proposal2, 1)]);

    // Now Carol starts two nodes by mistake, and equivocates. That crosses the FTT.
    let msg = create_message(&validators, 3, vote(true), &carol_kp);
    expect_no_gossip_block_finalized(sc.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 3, vote(false), &carol_kp);
    let outcomes = sc.handle_message(&mut rng, sender, msg, timestamp);
    assert!(outcomes.contains(&ProtocolOutcome::FttExceeded));
}

/// Tests that a `SyncState` message is periodically sent to a random peer.
#[test]
fn simple_consensus_sends_sync_state() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(50, 40, 10);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leader is Alice.
    let mut sc = new_test_simple_consensus(weights, vec![], &[alice_idx]);

    let alice_kp = Keypair::from(ALICE_SECRET_KEY.clone());
    let bob_kp = Keypair::from(BOB_SECRET_KEY.clone());
    let carol_kp = Keypair::from(CAROL_SECRET_KEY.clone());

    let timeout = sc.config.sync_state_interval.expect("request state timer");
    let sender = *ALICE_NODE_ID;
    let mut timestamp = Timestamp::from(100000);

    let proposal0 = Proposal {
        timestamp,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash0 = proposal0.hash();

    let outcomes = sc.handle_is_current(timestamp);
    expect_timer(&outcomes, timestamp + timeout, TIMER_ID_SYNC_PEER);

    timestamp += timeout;

    // The protocol state is empty and the SyncState should reflect that.
    let mut outcomes = sc.handle_timer(timestamp, TIMER_ID_SYNC_PEER, &mut rng);
    expect_timer(&outcomes, timestamp + timeout, TIMER_ID_SYNC_PEER);
    let mut msg_iter = remove_messages_to_random(&mut outcomes).into_iter();
    match (msg_iter.next(), msg_iter.next()) {
        (
            Some(Message::SyncState {
                round_id: 0,
                proposal_hash: None,
                proposal: false,
                first_validator_idx: _,
                echoes: 0,
                true_votes: 0,
                false_votes: 0,
                faulty: 0,
                instance_id: _,
            }),
            None,
        ) => {}
        (msg0, msg1) => panic!("unexpected messages: {:?}, {:?}", msg0, msg1),
    }

    timestamp += timeout;

    // Now we get a proposal and echo from Alice, one false vote from Bob, and Carol double-signs.
    let msg = create_message(&validators, 0, echo(hash0), &alice_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, proposal(&proposal0), &alice_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &bob_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(true), &carol_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &carol_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);

    // The next SyncState message must include all the new information.
    let mut outcomes = sc.handle_timer(timestamp, TIMER_ID_SYNC_PEER, &mut rng);
    expect_timer(&outcomes, timestamp + timeout, TIMER_ID_SYNC_PEER);
    let mut msg_iter = remove_messages_to_random(&mut outcomes).into_iter();
    match (msg_iter.next(), msg_iter.next()) {
        (
            Some(Message::SyncState {
                round_id: 0,
                proposal_hash: Some(hash),
                proposal: true,
                first_validator_idx,
                echoes,
                true_votes: 0,
                false_votes,
                faulty,
                instance_id: _,
            }),
            None,
        ) => {
            assert_eq!(hash0, hash);
            let mut faulty_iter = sc.iter_validator_bit_field(first_validator_idx, faulty);
            assert_eq!(Some(carol_idx), faulty_iter.next());
            assert_eq!(None, faulty_iter.next());
            let mut echoes_iter = sc.iter_validator_bit_field(first_validator_idx, echoes);
            assert_eq!(Some(alice_idx), echoes_iter.next());
            assert_eq!(None, echoes_iter.next());
            let mut false_iter = sc.iter_validator_bit_field(first_validator_idx, false_votes);
            assert_eq!(Some(bob_idx), false_iter.next());
            assert_eq!(None, false_iter.next());
        }
        (msg0, msg1) => panic!("unexpected messages: {:?}, {:?}", msg0, msg1),
    }
}

/// Tests that we respond to a `SyncState` message with the missing signatures.
#[test]
fn simple_consensus_handles_sync_state() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(50, 40, 10);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leader is Alice.
    let mut sc = new_test_simple_consensus(weights, vec![], &[alice_idx]);

    let alice_kp = Keypair::from(ALICE_SECRET_KEY.clone());
    let bob_kp = Keypair::from(BOB_SECRET_KEY.clone());
    let carol_kp = Keypair::from(CAROL_SECRET_KEY.clone());

    let sender = *ALICE_NODE_ID;
    let timestamp = Timestamp::from(100000);

    let proposal0 = Proposal {
        timestamp,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash0 = proposal0.hash();

    let proposal1 = Proposal::<ClContext> {
        timestamp,
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash1 = proposal1.hash();

    // We get a proposal, echo and true vote from Alice, one echo and false vote from Bob, and
    // Carol double-signs.
    let msg = create_message(&validators, 0, proposal(&proposal0), &alice_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, echo(hash0), &alice_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, echo(hash0), &bob_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &bob_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(true), &alice_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(true), &carol_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &carol_kp);
    sc.handle_message(&mut rng, sender, msg, timestamp);

    let first_validator_idx = ValidatorIndex(rng.gen_range(0..3));

    // The sender has everything we have except the proposal itself.
    let msg = Message::<ClContext>::SyncState {
        round_id: 0,
        proposal_hash: Some(hash0),
        proposal: false,
        first_validator_idx,
        echoes: sc.validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        true_votes: sc
            .validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        false_votes: sc
            .validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        faulty: sc.validator_bit_field(first_validator_idx, vec![carol_idx].into_iter()),
        instance_id: *sc.instance_id(),
    };
    let mut outcomes = sc.handle_message(&mut rng, sender, msg.serialize(), timestamp);
    let mut msgs = remove_targeted_messages(&validators, &mut outcomes);
    assert!(msgs.remove(&(sender, 0, ALICE_PUBLIC_KEY.clone(), proposal(&proposal0))));
    assert!(msgs.is_empty(), "unexpected messages: {:?}", msgs);
    expect_no_gossip_block_finalized(outcomes);

    // But if there are missing messages, these are sent back.
    let msg = Message::<ClContext>::SyncState {
        round_id: 0,
        proposal_hash: Some(hash1), // Wrong proposal!
        proposal: true,
        first_validator_idx,
        echoes: sc.validator_bit_field(first_validator_idx, vec![alice_idx].into_iter()),
        true_votes: sc
            .validator_bit_field(first_validator_idx, vec![bob_idx, alice_idx].into_iter()),
        false_votes: sc.validator_bit_field(first_validator_idx, vec![].into_iter()),
        faulty: sc.validator_bit_field(first_validator_idx, vec![].into_iter()),
        instance_id: *sc.instance_id(),
    };
    let mut outcomes = sc.handle_message(&mut rng, sender, msg.serialize(), timestamp);
    let mut msgs = remove_targeted_messages(&validators, &mut outcomes);
    // The sender's proposal hash is different from the one we have a quorum for:
    assert!(msgs.remove(&(sender, 0, ALICE_PUBLIC_KEY.clone(), echo(hash0))));
    assert!(msgs.remove(&(sender, 0, BOB_PUBLIC_KEY.clone(), echo(hash0))));
    // The sender has Alice's but not Bob's vote:
    assert!(msgs.remove(&(sender, 0, BOB_PUBLIC_KEY.clone(), vote(false))));
    // The sender doesn't know Carol is faulty:
    assert!(msgs.remove(&(sender, 0, CAROL_PUBLIC_KEY.clone(), vote(true))));
    assert!(msgs.remove(&(sender, 0, CAROL_PUBLIC_KEY.clone(), vote(false))));
    assert!(msgs.is_empty());
    expect_no_gossip_block_finalized(outcomes);
}

#[test]
fn test_validator_bit_field() {
    fn test_roundtrip(
        sc: &SimpleConsensus<ClContext>,
        first: u32,
        indexes: Vec<u32>,
        expected: Vec<u32>,
    ) {
        let field = sc.validator_bit_field(
            ValidatorIndex(first),
            indexes.iter().map(|i| ValidatorIndex(*i)),
        );
        let new_indexes: BTreeSet<u32> = sc
            .iter_validator_bit_field(ValidatorIndex(first), field)
            .map(|ValidatorIndex(i)| i)
            .collect();
        assert_eq!(expected.into_iter().collect::<BTreeSet<u32>>(), new_indexes);
    }

    let weights100: Vec<(PublicKey, U512)> = (0u8..100)
        .map(|i| {
            let sk = SecretKey::ed25519_from_bytes([i; SecretKey::ED25519_LENGTH]).unwrap();
            (PublicKey::from(&sk), U512::from(100))
        })
        .collect();

    let weights250: Vec<(PublicKey, U512)> = (0u8..250)
        .map(|i| {
            let sk = SecretKey::ed25519_from_bytes([i; SecretKey::ED25519_LENGTH]).unwrap();
            (PublicKey::from(&sk), U512::from(100))
        })
        .collect();

    let sc100 = new_test_simple_consensus(weights100, vec![], &[]);
    let sc250 = new_test_simple_consensus(weights250, vec![], &[]);

    test_roundtrip(&sc100, 50, vec![], vec![]);
    test_roundtrip(&sc250, 50, vec![], vec![]);
    test_roundtrip(&sc250, 200, vec![], vec![]);

    test_roundtrip(&sc100, 50, vec![0, 1, 49, 50, 99], vec![50, 99, 0, 1, 49]);
    test_roundtrip(&sc250, 50, vec![0, 49, 50, 177, 178, 249], vec![50, 177]);
    test_roundtrip(
        &sc250,
        200,
        vec![0, 77, 78, 200, 249],
        vec![200, 249, 0, 77],
    );
}

#[test]
fn test_quorum() {
    let (weights, validators) = abc_weights(66, 33, 1);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    let mut sc = new_test_simple_consensus(weights, vec![], &[]);

    // The threshold is the highest number that's below 2/3 of the weight.
    assert_eq!(66, sc.quorum_threshold().0);

    // So Alice alone with 66 is not a quorum, but with Carol she has 67.
    assert!(!sc.is_quorum(vec![].into_iter()));
    assert!(!sc.is_quorum(vec![alice_idx].into_iter()));
    assert!(sc.is_quorum(vec![alice_idx, carol_idx].into_iter()));
    assert!(sc.is_quorum(vec![alice_idx, bob_idx, carol_idx].into_iter()));

    // If Carol is known to be faulty, she counts towards every quorum.
    sc.mark_faulty(&CAROL_PUBLIC_KEY);

    // So now Alice's vote alone is sufficient.
    assert!(!sc.is_quorum(vec![].into_iter()));
    assert!(sc.is_quorum(vec![alice_idx].into_iter()));
}
