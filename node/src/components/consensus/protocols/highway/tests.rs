use std::{collections::BTreeSet, rc::Rc};

use datasize::DataSize;
use derive_more::Display;

use crate::{
    components::consensus::{
        candidate_block::CandidateBlock,
        cl_context::{ClContext, Keypair},
        consensus_protocol::{ConsensusProtocol, ProtocolOutcome},
        highway_core::{
            highway::{SignedWireUnit, Vertex, WireUnit},
            highway_testing,
            state::{self, tests::ALICE, Observation, Panorama},
            validators::{ValidatorIndex, ValidatorMap},
            State,
        },
        protocols::highway::HighwayMessage,
        tests::utils::{new_test_chainspec, ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_PUBLIC_KEY},
        traits::Context,
        HighwayProtocol,
    },
    crypto::asymmetric_key::PublicKey,
    testing::TestRng,
    types::{ProtoBlock, Timestamp},
};

#[derive(DataSize, Debug, Ord, PartialOrd, Clone, Display, Hash, Eq, PartialEq)]
pub(crate) struct NodeId(pub u8);

/// Returns a new `State` with `ClContext` parameters suitable for tests.
pub(crate) fn new_test_state(weights: &[state::Weight], seed: u64) -> State<ClContext> {
    let params = state::Params::new(
        seed,
        highway_testing::TEST_BLOCK_REWARD,
        highway_testing::TEST_BLOCK_REWARD / 5,
        14,
        19,
        4,
        u64::MAX,
        0.into(),
        Timestamp::from(u64::MAX),
        highway_testing::TEST_ENDORSEMENT_EVIDENCE_LIMIT,
    );
    state::State::new(weights, params, vec![])
}

const INSTANCE_ID_DATA: &[u8; 1] = &[123u8; 1];

pub(crate) fn new_test_highway_protocol<I>(
    init_slashed: I,
) -> Box<dyn ConsensusProtocol<NodeId, ClContext>>
where
    I: IntoIterator<Item = PublicKey>,
{
    let validators = vec![
        (*ALICE_PUBLIC_KEY, 100.into()),
        (*BOB_PUBLIC_KEY, 10.into()),
    ];
    let chainspec = new_test_chainspec(validators.clone());
    HighwayProtocol::<NodeId, ClContext>::new_boxed(
        ClContext::hash(INSTANCE_ID_DATA),
        validators.into_iter().collect(),
        &init_slashed.into_iter().collect(),
        &chainspec,
        None,
        0.into(),
        0,
    )
}

#[test]
fn test_highway_protocol_handle_message_parse_error() {
    // Build a highway_protocol for instrumentation
    let mut highway_protocol: Box<dyn ConsensusProtocol<NodeId, ClContext>> =
        new_test_highway_protocol(vec![]);

    let sender = NodeId(123);
    let msg = vec![];
    let mut rng = TestRng::new();
    let mut effects: Vec<ProtocolOutcome<NodeId, ClContext>> =
        highway_protocol.handle_message(sender.to_owned(), msg.to_owned(), false, &mut rng);

    assert_eq!(effects.len(), 1);

    let opt_protocol_outcome = effects.pop();

    match &opt_protocol_outcome {
        None => panic!("We just checked that effects has length 1!"),
        Some(ProtocolOutcome::InvalidIncomingMessage(invalid_msg, offending_sender, _err)) => {
            assert_eq!(
                invalid_msg, &msg,
                "Invalid message is not message that was sent."
            );
            assert_eq!(offending_sender, &sender, "Unexpected sender.")
        }
        Some(protocol_outcome) => panic!("Unexpected protocol outcome {:?}", protocol_outcome),
    }
}

pub(crate) const N: Observation<ClContext> = Observation::None;

#[test]
fn send_a_wire_unit_with_too_small_a_round_exp() {
    let creator: ValidatorIndex = ValidatorIndex(0);
    let state: State<ClContext> = new_test_state(&[100.into()], 0);
    let panorama: ValidatorMap<Observation<ClContext>> = Panorama::from(vec![N, N]);
    let seq_number = panorama.next_seq_num(&state, creator);
    let mut rng = TestRng::new();
    let wunit: WireUnit<ClContext> = WireUnit {
        panorama,
        creator,
        instance_id: ClContext::hash(INSTANCE_ID_DATA),
        value: None,
        seq_number,
        timestamp: 0.into(),
        round_exp: 0,
        endorsed: BTreeSet::new(),
    };
    let alice_keypair: Keypair = Keypair::from(Rc::new(ALICE_SECRET_KEY.clone()));
    let highway_message: HighwayMessage<ClContext> = HighwayMessage::NewVertex(Vertex::Unit(
        SignedWireUnit::new(wunit, &alice_keypair, &mut rng),
    ));
    let mut highway_protocol = new_test_highway_protocol(vec![]);
    let sender = NodeId(123);
    let msg = bincode::serialize(&highway_message).unwrap();
    let mut effects =
        highway_protocol.handle_message(sender.to_owned(), msg.to_owned(), false, &mut rng);
    assert_eq!(effects.len(), 1);

    let opt_protocol_outcome = effects.pop();
    match &opt_protocol_outcome {
        None => panic!("We just checked that effects has length 1!"),
        Some(ProtocolOutcome::InvalidIncomingMessage(invalid_msg, offending_sender, err)) => {
            assert_eq!(
                invalid_msg, &msg,
                "Invalid message is not message that was sent."
            );
            assert_eq!(offending_sender, &sender, "Unexpected sender.");
            assert!(
                format!("{:?}", err).starts_with(
                    "The vertex contains an invalid unit: `The round \
                     length exponent is less than the minimum allowed by \
                     the chain-spec.`"
                ),
                "Error message did not start as expected: {:?}",
                err
            )
        }
        Some(protocol_outcome) => panic!("Unexpected protocol outcome {:?}", protocol_outcome),
    }
}

#[test]
fn send_a_valid_wire_unit() {
    let creator: ValidatorIndex = ValidatorIndex(0);
    let state: State<ClContext> = new_test_state(&[100.into()], 0);
    let panorama: ValidatorMap<Observation<ClContext>> = Panorama::from(vec![N, N]);
    let seq_number = panorama.next_seq_num(&state, creator);
    let mut rng = TestRng::new();
    let wunit: WireUnit<ClContext> = WireUnit {
        panorama,
        creator,
        instance_id: ClContext::hash(INSTANCE_ID_DATA),
        value: Some(CandidateBlock::new(
            ProtoBlock::new(vec![], vec![], false),
            vec![],
        )),
        seq_number,
        timestamp: 0.into(),
        round_exp: 14,
        endorsed: BTreeSet::new(),
    };
    let alice_keypair: Keypair = Keypair::from(Rc::new(ALICE_SECRET_KEY.clone()));
    let highway_message: HighwayMessage<ClContext> = HighwayMessage::NewVertex(Vertex::Unit(
        SignedWireUnit::new(wunit, &alice_keypair, &mut rng),
    ));
    let mut highway_protocol = new_test_highway_protocol(vec![]);
    let sender = NodeId(123);
    let msg = bincode::serialize(&highway_message).unwrap();
    let mut effects = highway_protocol.handle_message(sender, msg, false, &mut rng);
    assert_eq!(effects.len(), 1);

    let opt_protocol_outcome = effects.pop();

    let mut new_effects = match &opt_protocol_outcome {
        None => panic!("We just checked that effects has length 1!"),
        Some(ProtocolOutcome::ValidateConsensusValue(node_id, candidate_block, timestamp)) => {
            assert_eq!(*node_id, NodeId(123));
            assert_eq!(*timestamp, Timestamp::from(0));
            highway_protocol.resolve_validity(candidate_block, true, &mut rng)
        }
        Some(protocol_outcome) => panic!("Unexpected protocol outcome {:?}", protocol_outcome),
    };

    assert_eq!(new_effects.len(), 2);

    let second_effect = new_effects
        .pop()
        .expect("We just checked that there were effects!");
    let first_effect = new_effects
        .pop()
        .expect("We just checked that there were 2 effects!");

    match first_effect {
        ProtocolOutcome::CreatedGossipMessage(_) => (),
        outcome => panic!("Unexpected protocol outcome: {:?}", outcome),
    }

    match second_effect {
        ProtocolOutcome::FinalizedBlock(_) => (),
        outcome => panic!("Unexpected protocol outcome: {:?}", outcome),
    }
}

#[test]
fn detect_doppelganger() {
    let creator: ValidatorIndex = ALICE;
    let state: State<ClContext> = new_test_state(&[100.into(), 100.into()], 0);
    let panorama: ValidatorMap<Observation<ClContext>> = Panorama::from(vec![N, N]);
    let seq_number = panorama.next_seq_num(&state, creator);
    let mut rng = TestRng::new();
    let instance_id = ClContext::hash(INSTANCE_ID_DATA);
    let round_exp = 14;
    let value = CandidateBlock::new(ProtoBlock::new(vec![], vec![], false), vec![]);
    let wunit: WireUnit<ClContext> = WireUnit {
        panorama,
        creator,
        instance_id,
        value: Some(value),
        seq_number,
        timestamp: 0.into(),
        round_exp,
        endorsed: BTreeSet::new(),
    };
    let alice_keypair: Keypair = Keypair::from(Rc::new(ALICE_SECRET_KEY.clone()));
    let highway_message: HighwayMessage<ClContext> = HighwayMessage::NewVertex(Vertex::Unit(
        SignedWireUnit::new(wunit, &alice_keypair, &mut rng),
    ));
    let mut highway_protocol = new_test_highway_protocol(vec![]);
    // Activate ALICE as validator.
    let _ =
        highway_protocol.activate_validator(*ALICE_PUBLIC_KEY, alice_keypair, Timestamp::zero());
    assert_eq!(highway_protocol.is_active(), true);
    let sender = NodeId(123);
    let msg = bincode::serialize(&highway_message).unwrap();
    // "Send" a message created by ALICE to an instance of Highway where she's an active validator.
    // An incoming unit, created by the same validator, should be properly detected as a
    // doppelganger.
    let effects = highway_protocol.handle_message(sender, msg, false, &mut rng);
    assert!(effects
        .iter()
        .any(|eff| matches!(eff, ProtocolOutcome::DoppelgangerDetected)));
    assert_eq!(highway_protocol.is_active(), false);
}
