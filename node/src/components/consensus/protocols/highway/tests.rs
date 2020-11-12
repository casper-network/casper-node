use std::iter;

use casper_execution_engine::{core::engine_state::GenesisAccount, shared::motes::Motes};

use crate::{
    components::{
        chainspec_loader::Chainspec,
        consensus::{
            cl_context::ClContext,
            consensus_protocol::{ConsensusProtocol, ProtocolOutcome},
            tests::mock_proto::NodeId,
            traits::Context,
            HighwayProtocol,
        },
    },
    crypto::asymmetric_key::{PublicKey, SecretKey},
    testing::TestRng,
    utils::Loadable,
};

fn new_test_chainspec(stakes: Vec<(PublicKey, u64)>) -> Chainspec {
    let mut chainspec = Chainspec::from_resources("local/chainspec.toml");
    chainspec.genesis.accounts = stakes
        .into_iter()
        .map(|(pk, stake)| {
            let motes = Motes::new(stake.into());
            GenesisAccount::new(pk.into(), pk.to_account_hash(), motes, motes)
        })
        .collect();
    chainspec.genesis.timestamp = 0.into();
    chainspec.genesis.highway_config.genesis_era_start_timestamp = chainspec.genesis.timestamp;
    chainspec
}

#[test]
fn test() {
    let mut rng = TestRng::new();

    // TODO: Unify with era_supervisor in components/consensus/era_supervisor/tests.rs
    let alice_sk = SecretKey::random(&mut rng);
    let alice_pk = PublicKey::from(&alice_sk);
    let bob_sk = SecretKey::random(&mut rng);
    let bob_pk = PublicKey::from(&bob_sk);

    // Build a highway_protocol for instrumentation
    let mut highway_protocol: Box<dyn ConsensusProtocol<NodeId, ClContext>> = {
        let chainspec = new_test_chainspec(vec![(alice_pk, 100)]);

        HighwayProtocol::<NodeId, ClContext>::new_boxed(
            ClContext::hash(&[123]),
            vec![
                (alice_pk, Motes::new(100.into())),
                (bob_pk, Motes::new(10.into())),
            ],
            &iter::once(bob_pk).collect(),
            &chainspec,
            None,
            0.into(),
            0,
        )
    };

    let sender = NodeId(123);
    let msg = vec![];
    let mut effects: Vec<ProtocolOutcome<NodeId, ClContext>> =
        highway_protocol.handle_message(sender.to_owned(), msg.to_owned(), false, &mut rng);

    assert_eq!(effects.len(), 1);

    let opt_protocol_outcome = effects.pop();

    match &opt_protocol_outcome {
        None => panic!("We just checked that effects has length 1!"),
        Some(ProtocolOutcome::InvalidIncomingMessage(invalid_msg, offending_sender, _err)) => {
            assert_eq!(
                invalid_msg, &msg,
                "Invalid message is not message that was sent.\
                  Invalid message in protocol outcome: {:?} ;\
                  Message that was sent: {:?}",
                invalid_msg, &msg
            );
            assert_eq!(
                offending_sender, &sender,
                "Unexpected sender.\
                  Sender according to protocol outcome: {:?} ;\
                  message that was sent: {:?}",
                offending_sender, &sender
            )
        }
        Some(protocol_outcome) => panic!("Unexpected protocol outcome {:?}", protocol_outcome),
    }
}
