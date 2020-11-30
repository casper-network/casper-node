use std::{collections::BTreeSet, iter, rc::Rc};

use casper_execution_engine::shared::motes::Motes;

use crate::{
    components::consensus::{
        cl_context::{ClContext, Keypair},
        consensus_protocol::{ConsensusProtocol, ProtocolOutcome},
        highway_core::{
            highway::{SignedWireUnit, Vertex, WireUnit},
            highway_testing,
            state::{self, Observation, Panorama},
            validators::{ValidatorIndex, ValidatorMap},
            State,
        },
        protocols::highway::HighwayMessage,
        tests::{
            mock_proto::NodeId,
            utils::{new_test_chainspec, ALICE_PUBLIC_KEY, ALICE_RAW_SECRET, BOB_PUBLIC_KEY},
        },
        traits::Context,
        HighwayProtocol,
    },
    crypto::asymmetric_key::SecretKey,
    testing::TestRng,
    types::Timestamp,
};

/// Returns a new `State` with `ClContext` parameters suitable for tests.
pub(crate) fn new_test_state(weights: &[state::Weight], seed: u64) -> State<ClContext> {
    let params = state::Params::new(
        seed,
        highway_testing::TEST_BLOCK_REWARD,
        highway_testing::TEST_BLOCK_REWARD / 5,
        4,
        19,
        4,
        u64::MAX,
        0.into(),
        Timestamp::from(u64::MAX),
    );
    state::State::new(weights, params, vec![])
}

const INSTANCE_ID_DATA: &[u8; 1] = &[123u8; 1];

pub(crate) fn new_test_highway_protocol() -> Box<dyn ConsensusProtocol<NodeId, ClContext>> {
    let chainspec = new_test_chainspec(vec![(*ALICE_PUBLIC_KEY, 100)]);
    HighwayProtocol::<NodeId, ClContext>::new_boxed(
        ClContext::hash(INSTANCE_ID_DATA),
        vec![
            (*ALICE_PUBLIC_KEY, Motes::new(100.into())),
            (*BOB_PUBLIC_KEY, Motes::new(10.into())),
        ],
        &iter::once(*BOB_PUBLIC_KEY).collect(),
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
        new_test_highway_protocol();

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
    let alice_keypair: Keypair =
        Rc::new(SecretKey::ed25519_from_bytes(&*ALICE_RAW_SECRET).unwrap()).into();
    let highway_message: HighwayMessage<ClContext> = HighwayMessage::NewVertex(Vertex::Unit(
        SignedWireUnit::new(wunit, &alice_keypair, &mut rng),
    ));
    let mut highway_protocol = new_test_highway_protocol();
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
