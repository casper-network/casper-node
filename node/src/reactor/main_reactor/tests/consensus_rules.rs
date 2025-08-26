use std::{collections::BTreeMap, sync::Arc};

use either::Either;
use tokio::time::{self};
use tracing::{error, info};

use casper_types::{
    system::auction::BidsExt, ConsensusProtocolName, EraId, PublicKey, SecretKey, Timestamp, U512,
};

use crate::{
    components::consensus::{self, NewBlockPayload},
    effect::{requests::NetworkRequest, EffectExt},
    protocol::Message,
    reactor::main_reactor::{
        tests::{
            configs_override::ConfigsOverride, fixture::TestFixture, initial_stakes::InitialStakes,
            switch_blocks::SwitchBlocks, ERA_TWO, ONE_MIN,
        },
        MainEvent,
    },
    types::BlockPayload,
};

#[tokio::test]
async fn run_equivocator_network() {
    let mut rng = crate::new_rng();

    let alice_secret_key = Arc::new(SecretKey::random(&mut rng));
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::new(SecretKey::random(&mut rng));
    let bob_public_key = PublicKey::from(&*bob_secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut rng));
    let charlie_public_key = PublicKey::from(&*charlie_secret_key);

    let mut stakes = BTreeMap::new();
    stakes.insert(
        alice_public_key.clone(),
        (U512::from(u64::MAX), U512::from(1)),
    );
    stakes.insert(
        bob_public_key.clone(),
        (U512::from(u64::MAX), U512::from(1)),
    );
    stakes.insert(
        charlie_public_key,
        (U512::from(u64::MAX), U512::from(u64::MAX)),
    );

    // Here's where things go wrong: Bob doesn't run a node at all, and Alice runs two!
    let secret_keys = vec![
        alice_secret_key.clone(),
        alice_secret_key,
        charlie_secret_key,
    ];

    // We configure the era to take 15 rounds. That should guarantee that the two nodes equivocate.
    let spec_override = ConfigsOverride {
        minimum_era_height: 10,
        consensus_protocol: ConsensusProtocolName::Highway,
        storage_multiplier: 2,
        ..Default::default()
    };

    let mut fixture =
        TestFixture::new_with_keys(rng, secret_keys, stakes.clone(), Some(spec_override)).await;

    let min_round_len = fixture.chainspec.core_config.minimum_block_time;
    let mut maybe_first_message_time = None;

    let mut alice_reactors = fixture
        .network
        .reactors_mut()
        .filter(|reactor| *reactor.inner().consensus().public_key() == alice_public_key);

    // Delay all messages to and from the first of Alice's nodes until three rounds after the first
    // message.  Further, significantly delay any incoming pings to avoid the node detecting the
    // doppelganger and deactivating itself.
    alice_reactors.next().unwrap().set_filter(move |event| {
        if crate::reactor::main_reactor::tests::is_ping(&event) {
            return Either::Left(time::sleep((min_round_len * 30).into()).event(move |_| event));
        }
        let now = Timestamp::now();
        match &event {
            MainEvent::ConsensusMessageIncoming(_) => {}
            MainEvent::NetworkRequest(
                NetworkRequest::SendMessage { payload, .. }
                | NetworkRequest::ValidatorBroadcast { payload, .. }
                | NetworkRequest::Gossip { payload, .. },
            ) if matches!(**payload, Message::Consensus(_)) => {}
            _ => return Either::Right(event),
        };
        let first_message_time = *maybe_first_message_time.get_or_insert(now);
        if now < first_message_time + min_round_len * 3 {
            return Either::Left(time::sleep(min_round_len.into()).event(move |_| event));
        }
        Either::Right(event)
    });

    // Significantly delay all incoming pings to the second of Alice's nodes.
    alice_reactors.next().unwrap().set_filter(move |event| {
        if crate::reactor::main_reactor::tests::is_ping(&event) {
            return Either::Left(time::sleep((min_round_len * 30).into()).event(move |_| event));
        }
        Either::Right(event)
    });

    drop(alice_reactors);

    let era_count = 4;

    let timeout = ONE_MIN * (era_count + 1) as u32;
    info!("Waiting for {} eras to end.", era_count);
    fixture
        .run_until_stored_switch_block_header(EraId::new(era_count - 1), timeout)
        .await;

    // network settled; select data to analyze
    let switch_blocks = SwitchBlocks::collect(fixture.network.nodes(), era_count);
    let mut era_bids = BTreeMap::new();
    for era in 0..era_count {
        era_bids.insert(era, switch_blocks.bids(fixture.network.nodes(), era));
    }

    // Since this setup sometimes produces no equivocation or an equivocation in era 2 rather than
    // era 1, we set an offset here.  If neither era has an equivocation, exit early.
    // TODO: Remove this once https://github.com/casper-network/casper-node/issues/1859 is fixed.
    for switch_block in &switch_blocks.headers {
        let era_id = switch_block.era_id();
        let count = switch_blocks.equivocators(era_id.value()).len();
        info!("equivocators in {}: {}", era_id, count);
    }
    let offset = if !switch_blocks.equivocators(1).is_empty() {
        0
    } else if !switch_blocks.equivocators(2).is_empty() {
        error!("failed to equivocate in era 1 - asserting equivocation detected in era 2");
        1
    } else {
        error!("failed to equivocate in era 1 or 2");
        return;
    };

    // Era 0 consists only of the genesis block.
    // In era 1, Alice equivocates. Since eviction takes place with a delay of one
    // (`auction_delay`) era, she is still included in the next era's validator set.
    let next_era_id = 1 + offset;

    assert_eq!(
        switch_blocks.equivocators(next_era_id),
        [alice_public_key.clone()]
    );
    let next_era_bids = era_bids.get(&next_era_id).expect("should have offset era");

    let next_era_alice = next_era_bids
        .validator_bid(&alice_public_key)
        .expect("should have Alice's offset bid");
    assert!(
        next_era_alice.inactive(),
        "Alice's bid should be inactive in offset era."
    );
    assert!(switch_blocks
        .next_era_validators(next_era_id)
        .contains_key(&alice_public_key));

    // In era 2 Alice is banned. Banned validators count neither as faulty nor inactive, even
    // though they cannot participate. In the next era, she will be evicted.
    let future_era_id = 2 + offset;
    assert_eq!(switch_blocks.equivocators(future_era_id), []);
    let future_era_bids = era_bids
        .get(&future_era_id)
        .expect("should have future era");
    let future_era_alice = future_era_bids
        .validator_bid(&alice_public_key)
        .expect("should have Alice's future bid");
    assert!(
        future_era_alice.inactive(),
        "Alice's bid should be inactive in future era."
    );
    assert!(!switch_blocks
        .next_era_validators(future_era_id)
        .contains_key(&alice_public_key));

    // In era 3 Alice is not a validator anymore and her bid remains deactivated.
    let era_3 = 3;
    if offset == 0 {
        assert_eq!(switch_blocks.equivocators(era_3), []);
        let era_3_bids = era_bids.get(&era_3).expect("should have era 3 bids");
        let era_3_alice = era_3_bids
            .validator_bid(&alice_public_key)
            .expect("should have Alice's era 3 bid");
        assert!(
            era_3_alice.inactive(),
            "Alice's bid should be inactive in era 3."
        );
        assert!(!switch_blocks
            .next_era_validators(era_3)
            .contains_key(&alice_public_key));
    }

    // Bob is inactive.
    assert_eq!(
        switch_blocks.inactive_validators(1),
        [bob_public_key.clone()]
    );
    assert_eq!(
        switch_blocks.inactive_validators(2),
        [bob_public_key.clone()]
    );

    for (era, bids) in era_bids {
        for (public_key, stake) in &stakes {
            let bid = bids
                .validator_bid(public_key)
                .expect("should have bid for public key {public_key} in era {era}");
            let staked_amount = bid.staked_amount();
            assert!(
                staked_amount >= stake.1,
                "expected stake {} for public key {} in era {}, found {}",
                staked_amount,
                public_key,
                era,
                stake.1
            );
        }
    }
}

// This test exercises a scenario in which a proposed block contains invalid accusations.
// Blocks containing no transactions or transfers used to be incorrectly marked as not needing
// validation even if they contained accusations, which opened up a security hole through which a
// malicious validator could accuse whomever they wanted of equivocating and have these
// accusations accepted by the other validators. This has been patched and the test asserts that
// such a scenario is no longer possible.
#[tokio::test]
async fn empty_proposed_block_validation_regression() {
    let initial_stakes = InitialStakes::AllEqual {
        count: 4,
        stake: 100,
    };
    let spec_override = ConfigsOverride {
        minimum_era_height: 15,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    let malicious_validator =
        PublicKey::from(fixture.node_contexts.first().unwrap().secret_key.as_ref());
    info!("Malicious validator: {}", malicious_validator);
    let everyone_else: Vec<_> = fixture
        .node_contexts
        .iter()
        .skip(1)
        .map(|node_context| PublicKey::from(node_context.secret_key.as_ref()))
        .collect();
    let malicious_id = fixture.node_contexts.first().unwrap().id;
    let malicious_runner = fixture.network.nodes_mut().get_mut(&malicious_id).unwrap();
    malicious_runner
        .reactor_mut()
        .inner_mut()
        .set_filter(move |event| match event {
            MainEvent::Consensus(consensus::Event::NewBlockPayload(NewBlockPayload {
                era_id,
                block_payload: _,
                block_context,
            })) => {
                info!("Accusing everyone else!");
                // We hook into the NewBlockPayload event to replace the block being proposed with
                // an empty one that accuses all the validators, except the malicious validator.
                Either::Right(MainEvent::Consensus(consensus::Event::NewBlockPayload(
                    NewBlockPayload {
                        era_id,
                        block_payload: Arc::new(BlockPayload::new(
                            BTreeMap::new(),
                            everyone_else.clone(),
                            Default::default(),
                            false,
                            1u8,
                        )),
                        block_context,
                    },
                )))
            }
            event => Either::Right(event),
        });

    info!("Waiting for the first era after genesis to end.");
    fixture.run_until_consensus_in_era(ERA_TWO, ONE_MIN).await;
    let switch_blocks = SwitchBlocks::collect(fixture.network.nodes(), 2);

    // Nobody actually double-signed. The accusations should have had no effect.
    assert_eq!(
        switch_blocks.equivocators(0),
        [],
        "expected no equivocators"
    );
    // If the malicious validator was the first proposer, all their Highway units might be invalid,
    // because they all refer to the invalid proposal, so they might get flagged as inactive. No
    // other validators should be considered inactive.
    match switch_blocks.inactive_validators(0) {
        [] => {}
        [inactive_validator] if malicious_validator == *inactive_validator => {}
        inactive => panic!("unexpected inactive validators: {:?}", inactive),
    }
}
