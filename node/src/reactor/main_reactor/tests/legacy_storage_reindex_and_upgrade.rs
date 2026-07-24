use std::{process::Command, sync::Arc};

use casper_types::{
    ActivationPoint, BlockV2, ChainspecRawBytes, EraId, PricingMode, ProtocolVersion, PublicKey,
    SecretKey, Transaction, U512,
};

use crate::{
    reactor::main_reactor::{
        tests::{fixture::TestFixture, Nodes, ONE_MIN},
        ReactorState,
    },
    types::transaction::transaction_v1_builder::TransactionV1Builder,
};

/// Path to a block store produced by an older node build that never persisted the
/// `block_height_index` / `switch_block_era_id_index` / `transaction_hash_index` databases (they
/// were kept purely in memory, rebuilt via a full header scan on every startup). Real blocks and
/// transactions across 3 eras, for 4 validators; captured once from `dev` and checked in as a
/// fixture -- see the module doc on [`boots_from_legacy_storage_reindexes_and_survives_upgrade`].
///
/// A single copy is committed, not one per node: the four validators that originally produced
/// this chain agree on identical blocks/transactions/global state by construction (that's what
/// consensus means), so any one of their block stores is a valid, fully interchangeable starting
/// point for any node in this test. (Their `storage.lmdb` files aren't literally byte-identical --
/// LMDB's on-disk page layout isn't deterministic even for equivalent logical content -- but
/// `data.lmdb`, the global state, was confirmed byte-identical across all four when this fixture
/// was captured.) Per-validator consensus vote-history (`unit_files`) is deliberately not
/// included: it's specific to a validator's own identity, so reusing one node's copy for another
/// would be wrong, and going without it is fine here since era_supervisor just creates the
/// directory fresh and starts with no prior voting history, same as any other new process.
const LEGACY_FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/reactor/main_reactor/tests/resources/legacy_storage_no_index"
);

fn fixed_secret_key(byte: u8) -> Arc<SecretKey> {
    Arc::new(SecretKey::ed25519_from_bytes([byte; SecretKey::ED25519_LENGTH]).unwrap())
}

fn transfer(from: &SecretKey, to: PublicKey, transfer_id: u64, chain_name: &str) -> Transaction {
    let mut txn = Transaction::from(
        TransactionV1Builder::new_transfer(30_000_000_000u64, None, to, Some(transfer_id))
            .unwrap()
            .with_initiator_addr(PublicKey::from(from))
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: 5,
                additional_computation_factor: 0,
            })
            .with_chain_name(chain_name.to_string())
            .build()
            .unwrap(),
    );
    txn.sign(from);
    txn
}

/// Boots a 4-validator network directly from a block store that predates the on-disk
/// `block_height_index` / `switch_block_era_id_index` / `transaction_hash_index` databases (real
/// blocks and transactions spanning 3 eras, produced by an older node build -- see
/// `LEGACY_FIXTURE_DIR`), and checks that:
///
/// 1. The reactor detects the missing indexes and rebuilds them from a header scan on startup
///    (rather than erroring, or silently behaving as if the chain were empty) -- exercised here by
///    reading a switch block for an era predating the fixture, and by looking up the execution
///    results of transactions baked into the fixture, both of which are served via the rebuilt
///    indexes.
/// 2. The resumed network is fully live: it accepts and executes a brand new transaction.
/// 3. It survives a protocol upgrade: restarting with a bumped protocol version and a future
///    activation point, the network continues past the activation era under the new protocol
///    version, produces the resulting immediate switch block, and keeps processing transactions
///    afterwards.
#[tokio::test]
async fn boots_from_legacy_storage_reindexes_and_survives_upgrade() {
    let secret_keys: Vec<Arc<SecretKey>> = (1..=4u8).map(fixed_secret_key).collect();
    let public_keys: Vec<PublicKey> = secret_keys
        .iter()
        .map(|key| PublicKey::from(key.as_ref()))
        .collect();
    let stakes = public_keys
        .iter()
        .cloned()
        .map(|public_key| {
            (
                public_key,
                (
                    U512::from(700_000_000_000_000_000u64),
                    U512::from(100_000_000_000_000u64),
                ),
            )
        })
        .collect();

    // Copy the single committed fixture into a fresh temp dir per node (so the test doesn't
    // mutate checked-in data, and each node gets its own independent, isolated copy to run
    // against -- see the doc on `LEGACY_FIXTURE_DIR` for why one copy is enough).
    let mut storage_dirs = Vec::new();
    for _ in 0..4 {
        let temp_dir = tempfile::tempdir().expect("should create temp dir");
        let status = Command::new("cp")
            .arg("-r")
            .arg(format!("{LEGACY_FIXTURE_DIR}/."))
            .arg(temp_dir.path())
            .status()
            .expect("failed to spawn cp");
        assert!(status.success(), "cp failed");
        storage_dirs.push(Arc::new(temp_dir));
    }

    let rng = casper_types::testing::TestRng::new();
    let mut fixture = TestFixture::new_with_keys_and_storage_dirs(
        rng,
        secret_keys.clone(),
        stakes,
        None,
        Some(storage_dirs),
    )
    .await;

    // The fixture was captured at height 6, era 3 -- confirm we resumed it rather than starting a
    // fresh genesis (which would be height 0, era 0).
    let resumed_at = fixture.highest_complete_block();
    assert!(
        resumed_at.height() >= 6,
        "should have resumed the pre-existing chain from the legacy fixture, not restarted genesis"
    );

    // The switch-block-era-id index was empty on disk; this only succeeds if it was rebuilt.
    let _ = fixture.switch_block(EraId::new(1));

    // The transaction-hash index was empty on disk too; look up the execution results of the
    // transactions actually baked into the fixture's blocks (rather than recomputing their
    // hashes, which aren't reproducible -- the builder stamps each with `Timestamp::now()`).
    let node_0 = fixture.node_contexts[0].id;
    let mut legacy_txn_count = 0;
    for height in 1..=resumed_at.height() {
        let Ok(block_v2) = BlockV2::try_from(fixture.get_block_by_height(height)) else {
            continue;
        };
        for txn_hash in block_v2.all_transactions() {
            let result = fixture
                .network
                .nodes()
                .get(&node_0)
                .expect("should have node 0")
                .main_reactor()
                .storage()
                .read_execution_result(txn_hash);
            assert!(
                result.is_some(),
                "transaction {txn_hash} from the legacy fixture should be readable via the \
                 rebuilt transaction_hash_index"
            );
            legacy_txn_count += 1;
        }
    }
    assert_eq!(
        legacy_txn_count, 3,
        "expected to find the 3 transfers baked into the legacy fixture"
    );

    // The resumed network should be fully live: it can accept and execute a new transaction.
    let chain_name = fixture.chainspec.network_config.name.clone();
    let pre_upgrade_txn = transfer(
        secret_keys[0].as_ref(),
        public_keys[1].clone(),
        100,
        &chain_name,
    );
    let pre_upgrade_txn_hash = pre_upgrade_txn.hash();
    fixture.inject_transaction(pre_upgrade_txn).await;
    fixture
        .run_until_executed_transaction(&pre_upgrade_txn_hash, ONE_MIN)
        .await;

    // Now drive it through a protocol upgrade, the same two-phase way a real deployment would:
    //
    // 1. Announce the upcoming upgrade (a new chainspec dropped next to the still-running old
    //    binary) via the upgrade watcher, without touching the running chainspec. The network keeps
    //    validating under the old protocol version until it reaches the switch block just before
    //    the activation era, at which point every node shuts down for upgrade.
    // 2. Only then restart every node -- reusing its (now-fully-indexed) storage dir -- with a
    //    chainspec whose `activation_point` is that same era. `activation_point` must never be a
    //    not-yet-reached era relative to a *live* chainspec: `ChainspecConsensusExt` treats it as
    //    "the era immediately after the most recent upgrade or restart", so setting it to a future
    //    era on a running chainspec (rather than one being restarted right at that point) trips
    //    `earliest_relevant_era`'s invariant in `EraSupervisor::create_required_eras`.
    //
    // No hard reset / global state update: this is an ordinary forward upgrade, not an emergency
    // rollback (contrast with `emergency_upgrade.rs`, which upgrades at an already-passed era).
    let activation_era = fixture
        .highest_complete_block()
        .era_id()
        .successor()
        .successor();
    let old_version = fixture.chainspec.protocol_config.version;
    let old_version_parts = old_version.value();
    let new_version = ProtocolVersion::from_parts(
        old_version_parts.major,
        old_version_parts.minor,
        old_version_parts.patch + 1,
    );

    fixture.schedule_upgrade(activation_era, new_version).await;
    // Wait not just for every node to report `ShutdownForUpgrade`, but for their local tips to
    // have actually converged on the same block first: a node can flip its reactor state to
    // `ShutdownForUpgrade` while a peer is still finishing executing/storing the last block or
    // two under the old protocol version, and stopping nodes non-atomically while they're still
    // staggered like that risks storing conflicting blocks at the same height across them.
    fixture
        .run_until(
            |nodes: &Nodes| {
                if !nodes
                    .values()
                    .all(|runner| runner.main_reactor().state == ReactorState::ShutdownForUpgrade)
                {
                    return false;
                }
                let mut tip_hashes = nodes.values().map(|runner| {
                    runner
                        .main_reactor()
                        .storage()
                        .get_highest_complete_block()
                        .ok()
                        .flatten()
                        .map(|block| *block.hash())
                });
                let Some(first) = tip_hashes.next() else {
                    return false;
                };
                tip_hashes.all(|hash| hash == first)
            },
            ONE_MIN,
        )
        .await;

    let mut new_chainspec = (*fixture.chainspec).clone();
    new_chainspec.protocol_config.version = new_version;
    new_chainspec.protocol_config.activation_point = ActivationPoint::EraId(activation_era);
    let new_chainspec = Arc::new(new_chainspec);
    let new_chainspec_raw_bytes: Arc<ChainspecRawBytes> = Arc::clone(&fixture.chainspec_raw_bytes);

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

    // The network should come back up, commit the upgrade, and produce the resulting immediate
    // switch block under the new protocol version.
    fixture
        .run_until_stored_switch_block_header(activation_era, ONE_MIN)
        .await;
    let post_upgrade_header = fixture.switch_block(activation_era);
    assert_eq!(post_upgrade_header.protocol_version(), new_version);

    // And the upgraded network is still fully live.
    let post_upgrade_txn = transfer(
        secret_keys[1].as_ref(),
        public_keys[2].clone(),
        101,
        &chain_name,
    );
    let post_upgrade_txn_hash = post_upgrade_txn.hash();
    fixture.inject_transaction(post_upgrade_txn).await;
    fixture
        .run_until_executed_transaction(&post_upgrade_txn_hash, ONE_MIN)
        .await;
}
