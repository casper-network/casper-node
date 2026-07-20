use std::{collections::BTreeMap, sync::Arc};

use casper_types::{
    ActivationPoint, ChainspecRawBytes, GlobalStateUpdate, ProtocolVersion, PublicKey, U512,
};

use crate::reactor::main_reactor::tests::{
    fixture::TestFixture, initial_stakes::InitialStakes, ERA_ONE, ERA_THREE, ERA_TWO, ONE_MIN,
};

/// Exercises an emergency protocol upgrade that requires "peeling" (hard-resetting) blocks
/// already stored under the old protocol version -- as would happen if a chain kept producing
/// blocks past the point an emergency fix needed to roll back to -- combined with a
/// `global_state_update` (an emergency validator-set confirmation).
///
/// This also verifies that the resulting immediate switch block still gets signed and enough
/// finality signatures gossiped around the (freshly restarted) network to be marked complete,
/// even though the protocol upgrade itself is committed before the reactor has any peers.
#[tokio::test]
async fn emergency_upgrade_requiring_block_peeling() {
    let initial_stakes = InitialStakes::AllEqual {
        count: 4,
        stake: 100_000_000_000,
    };
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    // Run the network well past era 2, as if the chain had kept producing blocks after the point
    // an emergency upgrade should have activated.
    fixture.run_until_consensus_in_era(ERA_THREE, ONE_MIN).await;

    // Build the "new" chainspec: a bumped protocol version, activating at era 2 (rolling back to
    // the era 1 switch block -- everything from era 2 onward, produced under the old protocol
    // version, must be peeled), with `hard_reset` set so storage actually performs that peel on
    // restart, plus an emergency `global_state_update` re-confirming the validator set.
    let mut new_chainspec = (*fixture.chainspec).clone();
    let old_version = new_chainspec.protocol_config.version;
    let old_version_parts = old_version.value();
    let new_version = ProtocolVersion::from_parts(
        old_version_parts.major,
        old_version_parts.minor,
        old_version_parts.patch + 1,
    );
    new_chainspec.protocol_config.version = new_version;
    new_chainspec.protocol_config.activation_point = ActivationPoint::EraId(ERA_TWO);
    new_chainspec.protocol_config.hard_reset = true;

    let validators: BTreeMap<_, _> = fixture
        .node_contexts
        .iter()
        .map(|node_context| {
            (
                PublicKey::from(node_context.secret_key.as_ref()),
                U512::from(100_000_000_000u64),
            )
        })
        .collect();
    new_chainspec.protocol_config.global_state_update = Some(GlobalStateUpdate {
        validators: Some(validators),
        entries: BTreeMap::new(),
    });
    let new_chainspec = Arc::new(new_chainspec);
    let new_chainspec_raw_bytes: Arc<ChainspecRawBytes> = Arc::clone(&fixture.chainspec_raw_bytes);

    // Restart every node with the new chainspec, reusing its storage directory -- so the
    // already-stored, now-stale, era-2+ blocks are still on disk to be peeled.
    let node_count = fixture.node_contexts.len();
    let node_contexts: Vec<_> = (0..node_count)
        .map(|_| fixture.remove_and_stop_node(0))
        .collect();
    for node_context in node_contexts {
        fixture
            .add_node_with_chainspec(
                node_context.secret_key,
                node_context.config,
                node_context.storage_dir,
                Arc::clone(&new_chainspec),
                Arc::clone(&new_chainspec_raw_bytes),
            )
            .await;
    }

    // The network should come back up, apply the upgrade, and continue producing (and
    // completing!) blocks -- proving the deferred sign+gossip mechanism for the immediate switch
    // block worked across the restart.
    fixture.run_until_block_height(3, ONE_MIN).await;

    for runner in fixture.network.nodes().values() {
        let storage = runner.main_reactor().storage();

        // The era 1 switch block (height 2) predates the hard-reset era and must be untouched.
        let era_one_switch_header = storage
            .read_block_header_by_height(2, false)
            .expect("should not error reading storage")
            .expect("era 1 switch block should still be present");
        assert_eq!(era_one_switch_header.era_id(), ERA_ONE);
        assert_eq!(era_one_switch_header.protocol_version(), old_version);

        // Any era-2+ blocks stored before the restart, under the OLD protocol version, must have
        // been peeled: the immediate switch block at height 3 must be the first block of era 2,
        // carrying the NEW protocol version.
        let post_upgrade_header = storage
            .read_block_header_by_height(3, false)
            .expect("should not error reading storage")
            .expect("post-upgrade immediate switch block should be present");
        assert_eq!(post_upgrade_header.era_id(), ERA_TWO);
        assert_eq!(post_upgrade_header.protocol_version(), new_version);
    }
}
