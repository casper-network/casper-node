use super::*;

use std::{collections::BTreeSet, sync::Arc};

use casper_types::{PublicKey, SecretKey, Timestamp, U512};
use tempfile::tempdir;
use tracing::info;

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
        EraMessage,
    },
    testing,
    types::BlockPayload,
};

const INSTANCE_ID_DATA: &[u8; 1] = &[123u8; 1];

/// Creates a new `Zug` instance.
///
/// The random seed is selected so that the leader sequence starts with `seq`.
pub(crate) fn new_test_zug<I1, I2, T>(
    weights: I1,
    init_faulty: I2,
    seq: &[ValidatorIndex],
) -> Zug<ClContext>
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
    let config = Config::default();
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
    Zug::<ClContext>::new(
        ClContext::hash(INSTANCE_ID_DATA),
        weights.into_iter().collect(),
        &init_faulty.into_iter().collect(),
        &None.into_iter().collect(),
        &chainspec,
        &config,
        None,
        start_timestamp,
        seed,
    )
}

/// Creates a `Message::Signed`.
fn create_message(
    validators: &Validators<PublicKey>,
    round_id: RoundId,
    content: Content<ClContext>,
    keypair: &Keypair,
) -> EraMessage<ClContext> {
    let validator_idx = validators.get_index(keypair.public_key()).unwrap();
    let instance_id = ClContext::hash(INSTANCE_ID_DATA);
    let signed_msg =
        SignedMessage::sign_new(round_id, instance_id, content, validator_idx, keypair);
    Message::Signed(signed_msg).into()
}

/// Creates a `Message::Proposal`
fn create_proposal_message(
    round_id: RoundId,
    proposal: &Proposal<ClContext>,
) -> EraMessage<ClContext> {
    Message::Proposal {
        round_id,
        instance_id: ClContext::hash(INSTANCE_ID_DATA),
        proposal: proposal.clone(),
    }
    .into()
}

/// Removes all `CreatedGossipMessage`s from `outcomes` and returns the messages, after
/// verifying the signatures and instance ID.
fn remove_gossip(
    validators: &Validators<PublicKey>,
    outcomes: &mut ProtocolOutcomes<ClContext>,
) -> Vec<Message<ClContext>> {
    let mut result = Vec::new();
    let expected_instance_id = ClContext::hash(INSTANCE_ID_DATA);
    outcomes.retain(|outcome| {
        let msg = match outcome {
            ProtocolOutcome::CreatedGossipMessage(EraMessage::Zug(msg)) => &**msg,
            _ => return true,
        };
        assert_eq!(*msg.instance_id(), expected_instance_id);
        if let Message::Signed(signed_msg) = msg {
            let public_key = validators
                .id(signed_msg.validator_idx)
                .expect("validator ID")
                .clone();
            assert!(signed_msg.verify_signature(&public_key));
        }
        result.push(msg.clone());
        false
    });
    result
}

/// Removes the expected signed message; returns `true` if found.
fn remove_signed(
    gossip: &mut Vec<Message<ClContext>>,
    expected_round_id: RoundId,
    expected_validator_idx: ValidatorIndex,
    expected_content: Content<ClContext>,
) -> bool {
    let maybe_pos = gossip.iter().position(|message| {
        if let Message::Signed(SignedMessage {
            round_id,
            instance_id: _,
            content,
            validator_idx,
            signature: _,
        }) = &message
        {
            *round_id == expected_round_id
                && *validator_idx == expected_validator_idx
                && *content == expected_content
        } else {
            false
        }
    });
    if let Some(pos) = maybe_pos {
        gossip.remove(pos);
        true
    } else {
        false
    }
}

/// Removes the expected proposal message; returns `true` if found.
fn remove_proposal(
    gossip: &mut Vec<Message<ClContext>>,
    expected_round_id: RoundId,
    expected_proposal: &Proposal<ClContext>,
) -> bool {
    let maybe_pos = gossip.iter().position(|message| {
        if let Message::Proposal {
            round_id,
            instance_id: _,
            proposal,
        } = &message
        {
            *round_id == expected_round_id && proposal == expected_proposal
        } else {
            false
        }
    });
    if let Some(pos) = maybe_pos {
        gossip.remove(pos);
        true
    } else {
        false
    }
}

/// Removes all `CreatedRequestToRandomPeer`s from `outcomes` and returns the deserialized messages.
fn remove_requests_to_random(
    outcomes: &mut ProtocolOutcomes<ClContext>,
) -> Vec<SyncRequest<ClContext>> {
    let mut result = Vec::new();
    let expected_instance_id = ClContext::hash(INSTANCE_ID_DATA);
    outcomes.retain(|outcome| {
        let msg: SyncRequest<ClContext> = match outcome {
            ProtocolOutcome::CreatedRequestToRandomPeer(msg) => {
                msg.clone().try_into_zug().expect("Zug request")
            }
            _ => return true,
        };
        assert_eq!(msg.instance_id, expected_instance_id);
        result.push(msg);
        false
    });
    result
}

/// Removes all `CreatedTargetedMessage`s from `outcomes` and returns the content of
/// all `Message::Signed`, after verifying the signatures.
fn remove_targeted_messages(
    validators: &Validators<PublicKey>,
    expected_peer: NodeId,
    outcomes: &mut ProtocolOutcomes<ClContext>,
) -> Vec<Message<ClContext>> {
    let mut result = Vec::new();
    let expected_instance_id = ClContext::hash(INSTANCE_ID_DATA);
    outcomes.retain(|outcome| {
        let (msg, peer) = match outcome {
            ProtocolOutcome::CreatedTargetedMessage(EraMessage::Zug(msg), peer) => (&**msg, *peer),
            _ => return true,
        };
        if peer != expected_peer {
            return true;
        }
        assert_eq!(*msg.instance_id(), expected_instance_id);
        if let Message::Signed(signed_msg) = msg {
            let public_key = validators
                .id(signed_msg.validator_idx)
                .expect("validator ID")
                .clone();
            assert!(signed_msg.verify_signature(&public_key));
        }
        result.push(msg.clone());
        false
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
            ProtocolOutcome::CreatedGossipMessage(EraMessage::Zug(msg)) => {
                panic!("unexpected gossip message {:?}", msg);
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
fn zug_no_fault() {
    testing::init_logging();
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(60, 30, 10);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();
    let sender = *ALICE_NODE_ID;

    let mut timestamp = Timestamp::from(100000);

    // The first round leaders are Bob, Alice, Alice, Carol, Carol.
    let leader_seq = &[bob_idx, alice_idx, alice_idx, carol_idx, carol_idx];
    let mut sc_c = new_test_zug(weights.clone(), vec![], leader_seq);
    let dir = tempdir().unwrap();
    sc_c.open_wal(dir.path().join("wal"), timestamp);

    let alice_kp = Keypair::from(ALICE_SECRET_KEY.clone());
    let bob_kp = Keypair::from(BOB_SECRET_KEY.clone());
    let carol_kp = Keypair::from(CAROL_SECRET_KEY.clone());

    sc_c.activate_validator(CAROL_PUBLIC_KEY.clone(), carol_kp, Timestamp::now(), None);

    let block_time = sc_c.params.min_block_time();
    let proposal_timeout = sc_c.proposal_timeout();

    let proposal0 = Proposal::<ClContext> {
        timestamp,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash0 = proposal0.hash();

    let proposal1 = Proposal {
        timestamp: proposal0.timestamp + block_time,
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash1 = proposal1.hash();

    let proposal2 = Proposal {
        timestamp: proposal1.timestamp + block_time,
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: Some(1),
        inactive: Some(Default::default()),
    };
    let hash2 = proposal2.hash();

    let proposal3 = Proposal {
        timestamp: proposal2.timestamp + block_time,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: Some(2),
        inactive: Some(Default::default()),
    };
    let hash3 = proposal3.hash();

    let proposal4 = Proposal::<ClContext> {
        timestamp: proposal3.timestamp + block_time,
        maybe_block: None,
        maybe_parent_round_id: Some(3),
        inactive: None,
    };
    let hash4 = proposal4.hash();

    // Carol's node joins a bit late, and gets some messages out of order.
    timestamp += block_time;

    // Alice makes a proposal in round 2 with parent in round 1. Alice and Bob echo it.
    let msg = create_message(&validators, 2, echo(hash2), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_proposal_message(2, &proposal2);
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
    let msg = create_message(&validators, 1, echo(hash1), &alice_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_proposal_message(1, &proposal1);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // Now Carol receives Bob's proposal in round 0. Carol echoes it.
    let msg = create_message(&validators, 0, echo(hash0), &bob_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_proposal_message(0, &proposal0);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_signed(&mut gossip, 0, carol_idx, echo(hash0)));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    expect_no_gossip_block_finalized(outcomes);

    timestamp += block_time;
    let msg = create_proposal_message(2, &proposal2);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // On timeout, Carol votes to make round 0 skippable.
    let mut outcomes = sc_c.handle_timer(timestamp, timestamp, TIMER_ID_UPDATE, &mut rng);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_signed(&mut gossip, 0, carol_idx, vote(false)));
    expect_no_gossip_block_finalized(outcomes);

    // Alice also echoes Bob's round 0 proposal, so it has a quorum and is accepted. With that round
    // 1 becomes current and Carol echoes Alice's proposal. That makes a quorum, but since round
    // 0 is not skippable round 1 is not yet accepted and thus round 2 is not yet current.
    let msg = create_message(&validators, 0, echo(hash0), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_signed(&mut gossip, 1, carol_idx, echo(hash1)));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    let timeout = timestamp + sc_c.proposal_timeout();
    expect_timer(&outcomes, timeout, TIMER_ID_UPDATE);

    // Bob votes false in round 0. That's not a quorum yet.
    let msg = create_message(&validators, 0, vote(false), &bob_kp);
    expect_no_gossip_block_finalized(sc_c.handle_message(&mut rng, sender, msg, timestamp));

    // On timeout, Carol votes to make round 1 skippable.
    // TODO: Come up with a better test scenario where timestamps are in order.
    let mut outcomes = sc_c.handle_timer(
        timestamp + proposal_timeout * 2,
        timestamp + proposal_timeout * 2,
        TIMER_ID_UPDATE,
        &mut rng,
    );
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_signed(&mut gossip, 1, carol_idx, vote(false)));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);

    // But with Alice's vote round 0 becomes skippable. That means rounds 1 and 2 are now accepted
    // and Carol votes for them. Since round 2 is already committed, both 1 and 2 are finalized.
    // Since round 2 became current, Carol echoes the proposal, too.
    let msg = create_message(&validators, 0, vote(false), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_signed(&mut gossip, 2, carol_idx, echo(hash2)));
    assert!(remove_signed(&mut gossip, 2, carol_idx, vote(true)));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    expect_finalized(&outcomes, &[(&proposal1, 0), (&proposal2, 1)]);
    expect_timer(&outcomes, timestamp + block_time, TIMER_ID_UPDATE);

    timestamp += block_time;

    // In round 3 Carol is the leader, so she creates a new block to propose.
    let mut outcomes = sc_c.handle_timer(timestamp, timestamp, TIMER_ID_UPDATE, &mut rng);
    let block_context = remove_create_new_block(&mut outcomes);
    expect_no_gossip_block_finalized(outcomes);
    assert_eq!(block_context.timestamp(), timestamp);
    assert_eq!(block_context.ancestor_values().len(), 2);

    let proposed_block = ProposedBlock::new(new_payload(false), block_context);
    let mut outcomes = sc_c.propose(proposed_block, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_proposal(&mut gossip, 3, &proposal3));
    assert!(remove_signed(&mut gossip, 3, carol_idx, echo(hash3)));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);

    timestamp += block_time;

    // Once Alice echoes Carol's proposal, she can go on to propose in round 4, too.
    // Since the round height is 3, the 4th proposal does not contain a block.
    let msg = create_message(&validators, 3, echo(hash3), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let mut gossip = remove_gossip(&validators, &mut outcomes);
    assert!(remove_signed(&mut gossip, 3, carol_idx, vote(true)));
    assert!(remove_proposal(&mut gossip, 4, &proposal4));
    assert!(remove_signed(&mut gossip, 4, carol_idx, echo(hash4)));
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);

    // Only when Alice also votes for the switch block is it finalized.
    assert!(!sc_c.finalized_switch_block());
    let msg = create_message(&validators, 3, vote(true), &alice_kp);
    let mut outcomes = sc_c.handle_message(&mut rng, sender, msg, timestamp);
    let gossip = remove_gossip(&validators, &mut outcomes);
    assert!(gossip.is_empty(), "unexpected gossip: {:?}", gossip);
    expect_finalized(&outcomes, &[(&proposal3, 2)]);
    assert!(sc_c.finalized_switch_block());

    info!("restoring protocol now");

    let mut zug = new_test_zug(weights, vec![], leader_seq);
    zug.open_wal(dir.path().join("wal"), timestamp);
    let outcomes = zug.handle_timer(timestamp, timestamp, TIMER_ID_UPDATE, &mut rng);
    let proposals123 = [(&proposal1, 0), (&proposal2, 1), (&proposal3, 2)];
    expect_finalized(&outcomes, &proposals123);
    assert!(zug.finalized_switch_block());
}

/// Tests that a faulty validator counts towards every quorum.
///
/// In this scenario Alice has 60% of the weight, Bob 10% and Carol 30%. Carol is offline and Bob is
/// faulty. Alice proposes a few blocks but can't finalize them alone. Once Bob double-signs, he
/// counts towards every quorum and Alice's messages suffice to finalize her blocks.
#[test]
fn zug_faults() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(60, 10, 30);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leaders are Carol, Alice, Alice.
    let mut zug = new_test_zug(weights, vec![], &[carol_idx, alice_idx, alice_idx]);

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
        timestamp: timestamp + zug.params.min_block_time(),
        maybe_block: Some(new_payload(true)),
        maybe_parent_round_id: Some(1),
        inactive: Some(iter::once(carol_idx).collect()),
    };
    let hash2 = proposal2.hash();

    timestamp += zug.params.min_block_time();

    // Alice makes sproposals in rounds 1 and 2, echoes and votes for them.
    let msg = create_message(&validators, 1, echo(hash1), &alice_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_proposal_message(1, &proposal1);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 1, vote(true), &alice_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, echo(hash2), &alice_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_proposal_message(2, &proposal2);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 2, vote(true), &alice_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));

    // Since Carol did not make a proposal Alice votes to make round 0 skippable.
    let msg = create_message(&validators, 0, vote(false), &alice_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));

    // Carol is offline and Alice alone does not have a quorum.
    // But if Bob equivocates, he counts towards every quorum, so the blocks get finalized.
    let msg = create_message(&validators, 3, vote(true), &bob_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 3, vote(false), &bob_kp);
    let outcomes = zug.handle_message(&mut rng, sender, msg, timestamp);
    expect_finalized(&outcomes, &[(&proposal1, 0), (&proposal2, 1)]);

    // Now Carol starts two nodes by mistake, and equivocates. That crosses the FTT.
    let msg = create_message(&validators, 3, vote(true), &carol_kp);
    expect_no_gossip_block_finalized(zug.handle_message(&mut rng, sender, msg, timestamp));
    let msg = create_message(&validators, 3, vote(false), &carol_kp);
    let outcomes = zug.handle_message(&mut rng, sender, msg, timestamp);
    assert!(outcomes.contains(&ProtocolOutcome::FttExceeded));
}

/// Tests that a `SyncRequest` message is periodically sent to a random peer.
#[test]
fn zug_sends_sync_request() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(50, 40, 10);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leader is Alice.
    let mut zug = new_test_zug(weights, vec![], &[alice_idx]);

    let alice_kp = Keypair::from(ALICE_SECRET_KEY.clone());
    let bob_kp = Keypair::from(BOB_SECRET_KEY.clone());
    let carol_kp = Keypair::from(CAROL_SECRET_KEY.clone());

    let timeout = zug.config.sync_state_interval.expect("request state timer");
    let sender = *ALICE_NODE_ID;
    let mut timestamp = Timestamp::from(100000);

    let proposal0 = Proposal::<ClContext> {
        timestamp,
        maybe_block: Some(new_payload(false)),
        maybe_parent_round_id: None,
        inactive: None,
    };
    let hash0 = proposal0.hash();

    let outcomes = zug.handle_is_current(timestamp);
    expect_timer(&outcomes, timestamp + timeout, TIMER_ID_SYNC_PEER);

    timestamp += timeout;

    // The protocol state is empty and the SyncRequest should reflect that.
    let mut outcomes = zug.handle_timer(timestamp, timestamp, TIMER_ID_SYNC_PEER, &mut rng);
    expect_timer(&outcomes, timestamp + timeout, TIMER_ID_SYNC_PEER);
    let mut msg_iter = remove_requests_to_random(&mut outcomes).into_iter();
    match (msg_iter.next(), msg_iter.next()) {
        (
            Some(SyncRequest {
                round_id: 0,
                proposal_hash: None,
                has_proposal: false,
                first_validator_idx: _,
                echoes: 0,
                true_votes: 0,
                false_votes: 0,
                active: 0,
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
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_proposal_message(0, &proposal0);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &bob_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(true), &carol_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &carol_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);

    // The next SyncRequest message must include all the new information.
    let mut outcomes = zug.handle_timer(timestamp, timestamp, TIMER_ID_SYNC_PEER, &mut rng);
    expect_timer(&outcomes, timestamp + timeout, TIMER_ID_SYNC_PEER);
    let mut msg_iter = remove_requests_to_random(&mut outcomes).into_iter();
    match (msg_iter.next(), msg_iter.next()) {
        (
            Some(SyncRequest {
                round_id: 0,
                proposal_hash: Some(hash),
                has_proposal: true,
                first_validator_idx,
                echoes,
                true_votes: 0,
                false_votes,
                active,
                faulty,
                instance_id: _,
            }),
            None,
        ) => {
            assert_eq!(hash0, hash);
            let mut faulty_iter = zug.iter_validator_bit_field(first_validator_idx, faulty);
            assert_eq!(Some(carol_idx), faulty_iter.next());
            assert_eq!(None, faulty_iter.next());
            let mut echoes_iter = zug.iter_validator_bit_field(first_validator_idx, echoes);
            assert_eq!(Some(alice_idx), echoes_iter.next());
            assert_eq!(None, echoes_iter.next());
            let mut false_iter = zug.iter_validator_bit_field(first_validator_idx, false_votes);
            assert_eq!(Some(bob_idx), false_iter.next());
            assert_eq!(None, false_iter.next());
            // When we marked Carol as faulty we removed her entry from the active list.
            let expected_active =
                zug.validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter());
            assert_eq!(active, expected_active);
        }
        (msg0, msg1) => panic!("unexpected messages: {:?}, {:?}", msg0, msg1),
    }
}

/// Tests that we respond to a `SyncRequest` message with the missing signatures.
#[test]
fn zug_handles_sync_request() {
    let mut rng = crate::new_rng();
    let (weights, validators) = abc_weights(50, 40, 10);
    let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
    let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
    let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

    // The first round leader is Alice.
    let mut zug = new_test_zug(weights.clone(), vec![], &[alice_idx]);

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
    let msg = create_message(&validators, 0, echo(hash0), &alice_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_proposal_message(0, &proposal0);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, echo(hash0), &bob_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &bob_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(true), &alice_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(true), &carol_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);
    let msg = create_message(&validators, 0, vote(false), &carol_kp);
    zug.handle_message(&mut rng, sender, msg, timestamp);

    let first_validator_idx = ValidatorIndex(rng.gen_range(0..3));

    // The sender has everything we have except the proposal itself.
    let msg = SyncRequest::<ClContext> {
        round_id: 0,
        proposal_hash: Some(hash0),
        has_proposal: false,
        first_validator_idx,
        echoes: zug.validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        true_votes: zug
            .validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        false_votes: zug
            .validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        active: zug.validator_bit_field(
            first_validator_idx,
            vec![alice_idx, bob_idx, carol_idx].into_iter(),
        ),
        faulty: zug.validator_bit_field(first_validator_idx, vec![carol_idx].into_iter()),
        instance_id: *zug.instance_id(),
    };
    let (outcomes, response) = zug.handle_request_message(&mut rng, sender, msg.into(), timestamp);
    assert_eq!(
        response
            .expect("response")
            .try_into_zug()
            .expect("Zug message"),
        Message::SyncResponse(SyncResponse {
            round_id: 0,
            proposal_or_hash: Some(Either::Left(proposal0)),
            echo_sigs: BTreeMap::new(),
            true_vote_sigs: BTreeMap::new(),
            false_vote_sigs: BTreeMap::new(),
            signed_messages: Vec::new(),
            evidence: Vec::new(),
            instance_id: *zug.instance_id(),
        })
    );
    expect_no_gossip_block_finalized(outcomes);

    // But if there are missing messages, these are sent back.
    let msg = SyncRequest::<ClContext> {
        round_id: 0,
        proposal_hash: Some(hash1), // Wrong proposal!
        has_proposal: true,
        first_validator_idx,
        echoes: zug.validator_bit_field(first_validator_idx, vec![alice_idx].into_iter()),
        true_votes: zug
            .validator_bit_field(first_validator_idx, vec![bob_idx, alice_idx].into_iter()),
        false_votes: zug.validator_bit_field(first_validator_idx, vec![].into_iter()),
        active: zug.validator_bit_field(first_validator_idx, vec![alice_idx, bob_idx].into_iter()),
        faulty: zug.validator_bit_field(first_validator_idx, vec![].into_iter()),
        instance_id: *zug.instance_id(),
    };
    let (mut outcomes, response) =
        zug.handle_request_message(&mut rng, sender, msg.into(), timestamp);
    assert_eq!(
        remove_targeted_messages(&validators, sender, &mut outcomes),
        vec![]
    );
    expect_no_gossip_block_finalized(outcomes);

    let sync_response = match response.expect("response").try_into_zug() {
        Ok(Message::SyncResponse(sync_response)) => sync_response,
        result => panic!("unexpected message: {:?}", result),
    };

    assert_eq!(sync_response.round_id, 0);
    assert_eq!(sync_response.proposal_or_hash, Some(Either::Right(hash0)));
    assert_eq!(
        sync_response.echo_sigs,
        zug.round(0).unwrap().echoes()[&hash0]
    );
    assert_eq!(sync_response.true_vote_sigs, BTreeMap::new());
    assert_eq!(sync_response.false_vote_sigs.len(), 1);
    assert_eq!(
        Some(sync_response.false_vote_sigs[&bob_idx]),
        zug.round(0).unwrap().votes(false)[bob_idx]
    );
    assert_eq!(sync_response.signed_messages, vec![]);
    assert_eq!(sync_response.evidence.len(), 1);
    match (&sync_response.evidence[0], &zug.faults[&carol_idx]) {
        (
            (signed_msg, content2, sig2),
            Fault::Direct(expected_signed_msg, expected_content2, expected_sig2),
        ) => {
            assert_eq!(signed_msg, expected_signed_msg);
            assert_eq!(content2, expected_content2);
            assert_eq!(sig2, expected_sig2);
        }
        (evidence, fault) => panic!("unexpected evidence: {:?}, {:?}", evidence, fault),
    }

    // Create a new instance that doesn't have any data yet, let it send two sync requests to Zug,
    // and handle the responses.
    let mut zug2 = new_test_zug(weights, vec![], &[alice_idx]);
    for _ in 0..2 {
        let mut outcomes = zug2.handle_timer(timestamp, timestamp, TIMER_ID_SYNC_PEER, &mut rng);
        let msg = loop {
            if let ProtocolOutcome::CreatedRequestToRandomPeer(payload) =
                outcomes.pop().expect("expected request to random peer")
            {
                break payload;
            }
        };
        let (_outcomes, response) = zug.handle_request_message(&mut rng, sender, msg, timestamp);
        if let Some(msg) = response {
            let mut _outcomes = zug2.handle_message(&mut rng, sender, msg, timestamp);
        }
    }

    // They should be synced up now:
    assert_eq!(zug.rounds, zug2.rounds);
    assert_eq!(zug.faults, zug2.faults);
    assert_eq!(zug.active, zug2.active);
}

#[test]
fn test_validator_bit_field() {
    fn test_roundtrip(zug: &Zug<ClContext>, first: u32, indexes: Vec<u32>, expected: Vec<u32>) {
        let field = zug.validator_bit_field(
            ValidatorIndex(first),
            indexes.iter().map(|i| ValidatorIndex(*i)),
        );
        let new_indexes: BTreeSet<u32> = zug
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

    let sc100 = new_test_zug(weights100, vec![], &[]);
    let sc250 = new_test_zug(weights250, vec![], &[]);

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
    // Alice has almost 2/3 of the weight, Bob almost 1/3, and Carol 1.
    let weights_without_overflow = (66, 33, 1);
    // A similar distribution, but the quorum calculation would overflow if it naively added the
    // total weight to the ftt.
    let weights_with_overflow = (1 << 63, 1 << 62, 1);
    for (a, b, c) in [weights_without_overflow, weights_with_overflow] {
        let (weights, validators) = abc_weights(a, b, c);
        let alice_idx = validators.get_index(&*ALICE_PUBLIC_KEY).unwrap();
        let bob_idx = validators.get_index(&*BOB_PUBLIC_KEY).unwrap();
        let carol_idx = validators.get_index(&*CAROL_PUBLIC_KEY).unwrap();

        let mut zug = new_test_zug(weights, vec![], &[]);

        // The threshold is the highest number that's below 2/3 of the weight.
        assert_eq!(a, zug.quorum_threshold().0);

        // Alice alone is not a quorum, but with Carol she is.
        assert!(!zug.is_quorum(vec![].into_iter()));
        assert!(!zug.is_quorum(vec![alice_idx].into_iter()));
        assert!(zug.is_quorum(vec![alice_idx, carol_idx].into_iter()));
        assert!(zug.is_quorum(vec![alice_idx, bob_idx, carol_idx].into_iter()));

        // If Carol is known to be faulty, she counts towards every quorum.
        zug.mark_faulty(&CAROL_PUBLIC_KEY);

        // So now Alice's vote alone is sufficient.
        assert!(!zug.is_quorum(vec![].into_iter()));
        assert!(zug.is_quorum(vec![alice_idx].into_iter()));
    }
}

#[test]
fn update_proposal_timeout() {
    macro_rules! assert_approx {
        ($val0:expr, $val1:expr) => {
            let v0: f64 = $val0;
            let v1: f64 = $val1;
            let diff = (v1 - v0).abs();
            let min = v1.abs().min(v0.abs());
            assert!(diff < min * 0.1, "not approximately equal: {}, {}", v0, v1);
        };
    }

    let mut rng = crate::new_rng();

    let (weights, _validators) = abc_weights(1, 2, 3);
    let mut zug = new_test_zug(weights, vec![], &[]);
    let _outcomes = zug.handle_timer(
        Timestamp::from(100000),
        Timestamp::from(100000),
        TIMER_ID_UPDATE,
        &mut rng,
    );

    let round_start = zug.current_round_start;
    let grace_factor = zug.config.proposal_grace_period as f64 / 100.0 + 1.0;
    let inertia = zug.config.proposal_timeout_inertia;
    let initial_timeout = zug.config.proposal_timeout.millis() as f64 * grace_factor;

    let timeout = zug.proposal_timeout().millis() as f64;

    assert_approx!(initial_timeout, timeout);

    // Within 2 * inertia blocks the timeout should double and go back down again, if rounds
    // without proposals come before rounds with fast proposals and the fraction of rounds with
    // fast proposals is (1 + ftt) / 2, i.e. 2/3.
    let fail_rounds = (inertia as f64 * 2.0 / 3.0).round() as u16;
    let success_rounds = 2 * inertia - fail_rounds;
    for _ in 0..fail_rounds {
        zug.update_proposal_timeout(round_start + TimeDiff::from_seconds(10000));
    }
    assert_approx!(
        2.0 * initial_timeout,
        zug.proposal_timeout().millis() as f64
    );
    for _ in 0..success_rounds {
        zug.update_proposal_timeout(round_start + TimeDiff::from_millis(1));
    }
    assert_approx!(initial_timeout, zug.proposal_timeout().millis() as f64);

    // If the proposal delay is consistently t, the timeout will settle on t * grace_factor
    // within 2 * inertia rounds.
    let min_delay = (zug.proposal_timeout().millis() as f64 / grace_factor) as u64;
    for _ in 0..10 {
        let delay = TimeDiff::from_millis(rng.gen_range(min_delay..(min_delay * 2)));
        for _ in 0..(2 * inertia) {
            zug.update_proposal_timeout(round_start + delay);
        }
        assert_eq!(
            delay.millis() as f64 * grace_factor,
            zug.proposal_timeout().millis() as f64
        );
    }
}
