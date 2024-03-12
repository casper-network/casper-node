use std::iter;

use prometheus::Registry;
use rand::Rng;

use casper_types::{testing::TestRng, EraId, SecretKey, TimeDiff};

use super::*;
use crate::{
    effect::announcements::DeployBufferAnnouncement::{self, DeploysExpired},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::{Block, FinalizedBlock},
    utils,
};

enum DeployType {
    Transfer,
    Standard,
    Random,
}

// Generates valid deploys
fn create_valid_deploys(
    rng: &mut TestRng,
    size: usize,
    deploy_type: DeployType,
    strict_timestamp: Option<Timestamp>,
    with_ttl: Option<TimeDiff>,
) -> Vec<Deploy> {
    let mut deploys = Vec::with_capacity(size);

    for _ in 0..size {
        let deploy_ttl = match with_ttl {
            Some(ttl) => ttl,
            None => TimeDiff::from_seconds(rng.gen_range(30..100)),
        };
        let deploy_timestamp = match strict_timestamp {
            Some(timestamp) => timestamp,
            None => Timestamp::now(),
        };
        match deploy_type {
            DeployType::Transfer => {
                deploys.push(Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
                    rng,
                    deploy_timestamp,
                    deploy_ttl,
                ));
            }
            DeployType::Standard => {
                if strict_timestamp.is_some() {
                    unimplemented!();
                }
                let deploy = Deploy::random_with_valid_session_package_by_name(rng);
                assert!(deploy.is_valid().is_ok());
                deploys.push(deploy);
            }
            DeployType::Random => {
                deploys.push(Deploy::random_with_timestamp_and_ttl(
                    rng,
                    deploy_timestamp,
                    deploy_ttl,
                ));
            }
        }
    }

    deploys
}

/// Checks sizes of the deploy_buffer containers. Also checks the metrics recorded.
#[track_caller]
fn assert_container_sizes(
    deploy_buffer: &DeployBuffer,
    expected_buffer: usize,
    expected_dead: usize,
    expected_held: usize,
) {
    assert_eq!(
        deploy_buffer.buffer.len(),
        expected_buffer,
        "buffer.len {} != expected {}",
        deploy_buffer.buffer.len(),
        expected_buffer
    );
    assert_eq!(
        deploy_buffer.dead.len(),
        expected_dead,
        "dead.len {} != expected {}",
        deploy_buffer.dead.len(),
        expected_dead
    );
    let hold_len = deploy_buffer
        .hold
        .values()
        .map(|deploys| deploys.len())
        .sum::<usize>();
    assert_eq!(
        hold_len, expected_held,
        "hold.len {} != expected {}",
        hold_len, expected_held
    );
    assert_eq!(
        deploy_buffer.metrics.total_deploys.get(),
        expected_buffer as i64,
        "metrics total {} != expected {}",
        deploy_buffer.metrics.total_deploys.get(),
        expected_buffer,
    );
    assert_eq!(
        deploy_buffer.metrics.held_deploys.get(),
        expected_held as i64,
        "metrics held {} != expected {}",
        deploy_buffer.metrics.held_deploys.get(),
        expected_held,
    );
    assert_eq!(
        deploy_buffer.metrics.dead_deploys.get(),
        expected_dead as i64,
        "metrics dead {} != expected {}",
        deploy_buffer.metrics.dead_deploys.get(),
        expected_dead,
    );
}

#[test]
fn register_deploy_and_check_size() {
    let mut rng = TestRng::new();
    let mut deploy_buffer =
        DeployBuffer::new(DeployConfig::default(), Config::default(), &Registry::new()).unwrap();

    // Try to register valid deploys
    let num_valid_deploys: usize = rng.gen_range(50..500);
    let valid_deploys =
        create_valid_deploys(&mut rng, num_valid_deploys, DeployType::Random, None, None);
    valid_deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, valid_deploys.len(), 0, 0);

    // Try to register a duplicate deploy
    let duplicate_deploy = valid_deploys
        .get(rng.gen_range(0..num_valid_deploys))
        .unwrap()
        .clone();
    deploy_buffer.register_deploy(duplicate_deploy);
    assert_container_sizes(&deploy_buffer, valid_deploys.len(), 0, 0);

    // Insert deploy without footprint
    let bad_deploy = Deploy::random_without_payment_amount(&mut rng);
    deploy_buffer.register_deploy(bad_deploy);
    assert_container_sizes(&deploy_buffer, valid_deploys.len(), 0, 0);
}

#[test]
fn register_block_with_valid_deploys() {
    let mut rng = TestRng::new();
    let mut deploy_buffer =
        DeployBuffer::new(DeployConfig::default(), Config::default(), &Registry::new()).unwrap();

    let deploys = create_valid_deploys(&mut rng, 10, DeployType::Random, None, None);
    let block = Block::random_with_deploys(&mut rng, deploys.iter());

    deploy_buffer.register_block(&block);
    assert_container_sizes(&deploy_buffer, deploys.len(), deploys.len(), 0);
}

#[test]
fn register_finalized_block_with_valid_deploys() {
    let mut rng = TestRng::new();
    let mut deploy_buffer =
        DeployBuffer::new(DeployConfig::default(), Config::default(), &Registry::new()).unwrap();

    let deploys = create_valid_deploys(&mut rng, 10, DeployType::Random, None, None);
    let block = FinalizedBlock::random_with_deploys(&mut rng, deploys.iter());

    deploy_buffer.register_block_finalized(&block);
    assert_container_sizes(&deploy_buffer, deploys.len(), deploys.len(), 0);
}

#[test]
fn get_proposable_deploys() {
    let mut rng = TestRng::new();
    let mut deploy_buffer =
        DeployBuffer::new(DeployConfig::default(), Config::default(), &Registry::new()).unwrap();

    // populate deploy buffer with some deploys
    let deploys = create_valid_deploys(&mut rng, 50, DeployType::Random, None, None);
    deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, deploys.len(), 0, 0);

    // Create a block with some deploys and register it with the deploy_buffer
    let block_deploys = create_valid_deploys(&mut rng, 10, DeployType::Random, None, None);
    let block = FinalizedBlock::random_with_deploys(&mut rng, block_deploys.iter());
    deploy_buffer.register_block_finalized(&block);
    assert_container_sizes(
        &deploy_buffer,
        deploys.len() + block_deploys.len(),
        block_deploys.len(),
        0,
    );

    // Check which deploys are proposable. Should return the deploys that were not included in the
    // block since those should be dead.
    let proposable = deploy_buffer.proposable();
    assert_eq!(proposable.len(), deploys.len());
    let proposable_deploy_hashes: HashSet<_> =
        proposable.iter().map(|(dh, _)| dh.deploy_hash()).collect();
    for deploy in deploys.iter() {
        assert!(proposable_deploy_hashes.contains(deploy.hash()));
    }

    let timestamp = Timestamp::now();
    let expiry = timestamp.saturating_add(TimeDiff::from_seconds(1));
    let appendable_block = deploy_buffer.appendable_block(timestamp, expiry);
    assert_eq!(deploy_buffer.hold.len(), 1);
    assert_container_sizes(
        &deploy_buffer,
        deploys.len() + block_deploys.len(),
        block_deploys.len(),
        appendable_block.deploy_and_transfer_set().len(),
    );

    // Check that held blocks are not proposable
    let proposable = deploy_buffer.proposable();
    assert_eq!(
        proposable.len(),
        deploys.len() - appendable_block.deploy_and_transfer_set().len()
    );
    for deploy in proposable.iter() {
        assert!(!appendable_block
            .deploy_and_transfer_set()
            .contains(deploy.0.deploy_hash()));
    }
}

#[test]
fn get_appendable_block_with_native_transfers() {
    let mut rng = TestRng::new();
    let deploy_config = DeployConfig {
        block_max_deploy_count: 10,
        block_max_transfer_count: 200,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut deploy_buffer,
        DeployType::Transfer,
        deploy_config.block_max_transfer_count as usize,
    );
}

#[test]
fn get_appendable_block_with_standard_deploys() {
    let mut rng = TestRng::new();
    let deploy_config = DeployConfig {
        block_max_deploy_count: 10,
        block_max_transfer_count: 200,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut deploy_buffer,
        DeployType::Standard,
        deploy_config.block_max_deploy_count as usize,
    );
}

#[test]
fn get_appendable_block_with_random_deploys() {
    let mut rng = TestRng::new();
    let deploy_config = DeployConfig {
        block_max_deploy_count: 10,
        block_max_transfer_count: 200,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut deploy_buffer,
        DeployType::Random,
        deploy_config.block_max_transfer_count as usize,
    );
}

fn get_appendable_block(
    rng: &mut TestRng,
    deploy_buffer: &mut DeployBuffer,
    deploy_type: DeployType,
    deploy_limit: usize,
) {
    // populate deploy buffer with more transfers than a block can fit
    let deploys = create_valid_deploys(rng, deploy_limit + 50, deploy_type, None, None);
    deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(deploy_buffer, deploys.len(), 0, 0);

    let timestamp = Timestamp::now();
    let expiry = timestamp.saturating_add(TimeDiff::from_seconds(1));
    let appendable_block = deploy_buffer.appendable_block(timestamp, expiry);
    assert!(appendable_block.deploy_and_transfer_set().len() <= deploy_limit,);
    assert_eq!(deploy_buffer.hold.len(), 1);
    assert_container_sizes(
        deploy_buffer,
        deploys.len(),
        0,
        appendable_block.deploy_and_transfer_set().len(),
    );
}

#[test]
fn register_deploys_and_blocks() {
    let mut rng = TestRng::new();
    let mut deploy_buffer =
        DeployBuffer::new(DeployConfig::default(), Config::default(), &Registry::new()).unwrap();

    // try to register valid deploys
    let num_valid_deploys: usize = rng.gen_range(50..500);
    let valid_deploys =
        create_valid_deploys(&mut rng, num_valid_deploys, DeployType::Random, None, None);
    valid_deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, valid_deploys.len(), 0, 0);

    // register a block with deploys
    let block_deploys = create_valid_deploys(&mut rng, 5, DeployType::Random, None, None);
    let block = Block::random_with_deploys(&mut rng, block_deploys.iter());
    deploy_buffer.register_block(&block);
    assert_container_sizes(
        &deploy_buffer,
        block_deploys.len() + valid_deploys.len(),
        block_deploys.len(),
        0,
    );

    // try to register the deploys of the block again. Should not work since those deploys are dead.
    block_deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(
        &deploy_buffer,
        block_deploys.len() + valid_deploys.len(),
        block_deploys.len(),
        0,
    );

    let pre_proposal_timestamp = Timestamp::now();

    let timestamp = Timestamp::now();
    let expiry = timestamp.saturating_add(TimeDiff::from_seconds(1));
    let appendable_block = deploy_buffer.appendable_block(timestamp, expiry);
    assert_eq!(deploy_buffer.hold.len(), 1);
    assert_container_sizes(
        &deploy_buffer,
        block_deploys.len() + valid_deploys.len(),
        block_deploys.len(),
        appendable_block.deploy_and_transfer_set().len(),
    );

    // try to register held deploys again.
    let mut held_deploys = valid_deploys
        .iter()
        .cloned()
        .filter(|deploy| {
            appendable_block
                .deploy_and_transfer_set()
                .contains(deploy.hash())
        })
        .peekable();
    assert!(held_deploys.peek().is_some());
    held_deploys.for_each(|deploy| deploy_buffer.register_deploy(deploy));
    assert_container_sizes(
        &deploy_buffer,
        block_deploys.len() + valid_deploys.len(),
        block_deploys.len(),
        appendable_block.deploy_and_transfer_set().len(),
    );

    // test if deploys held for proposed blocks which did not get finalized in time
    // are eligible again
    let block = FinalizedBlock::random_with_specifics(
        &mut rng,
        EraId::from(2),
        25,
        false,
        pre_proposal_timestamp,
        None,
    );
    deploy_buffer.register_block_finalized(&block);
    assert_container_sizes(
        &deploy_buffer,
        block_deploys.len() + valid_deploys.len() + block.deploy_and_transfer_hashes().count(),
        block_deploys.len() + block.deploy_and_transfer_hashes().count(),
        0,
    );
}

#[test]
fn should_have_one_bucket_per_distinct_body_hash() {
    let mut rng = TestRng::new();
    let max_deploy_count = 2;
    let max_transfer_count = 0;
    let deploy_config = DeployConfig {
        block_max_deploy_count: max_deploy_count,
        block_max_transfer_count: max_transfer_count,
        block_max_approval_count: max_deploy_count + max_transfer_count,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();

    let secret_key1 = SecretKey::random(&mut rng);
    let ttl = TimeDiff::from_seconds(30);
    let deploy1 = Deploy::random_contract_by_name(
        &mut rng,
        Some(secret_key1),
        None,
        None,
        Some(Timestamp::now()),
        Some(ttl),
    );
    let deploy1_body_hash = *deploy1.header().body_hash();
    deploy_buffer.register_deploy(deploy1);

    let secret_key2 = SecretKey::random(&mut rng); // different signer
    let deploy2 = Deploy::random_contract_by_name(
        &mut rng,
        Some(
            SecretKey::from_pem(secret_key2.to_pem().expect("should pemify"))
                .expect("should un-pemify"),
        ),
        None,
        None,
        Some(Timestamp::now()), // different timestamp
        Some(ttl),
    );
    assert_eq!(
        &deploy1_body_hash,
        deploy2.header().body_hash(),
        "1 & 2 should have same body hashes"
    );
    deploy_buffer.register_deploy(deploy2);

    let buckets = deploy_buffer.buckets();
    assert!(buckets.len() == 1, "should be 1 bucket");

    let deploy3 = Deploy::random_contract_by_name(
        &mut rng,
        Some(
            SecretKey::from_pem(secret_key2.to_pem().expect("should pemify"))
                .expect("should un-pemify"),
        ),
        None,
        None,
        Some(Timestamp::now()), // different timestamp
        Some(ttl),
    );
    assert_eq!(
        &deploy1_body_hash,
        deploy3.header().body_hash(),
        "1 & 3 should have same body hashes"
    );
    deploy_buffer.register_deploy(deploy3);
    let buckets = deploy_buffer.buckets();
    assert!(buckets.len() == 1, "should still be 1 bucket");

    let deploy4 = Deploy::random_contract_by_name(
        &mut rng,
        Some(
            SecretKey::from_pem(secret_key2.to_pem().expect("should pemify"))
                .expect("should un-pemify"),
        ),
        Some("some other contract name".to_string()),
        None,
        Some(Timestamp::now()), // different timestamp
        Some(ttl),
    );
    assert_ne!(
        &deploy1_body_hash,
        deploy4.header().body_hash(),
        "1 & 4 should have different body hashes"
    );
    deploy_buffer.register_deploy(deploy4);
    let buckets = deploy_buffer.buckets();
    assert!(buckets.len() == 2, "should be 2 buckets");

    let transfer5 = Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
        &mut rng,
        Timestamp::now(),
        ttl,
    );
    assert_ne!(
        &deploy1_body_hash,
        transfer5.header().body_hash(),
        "1 & 5 should have different body hashes"
    );
    deploy_buffer.register_deploy(transfer5);
    let buckets = deploy_buffer.buckets();
    assert!(buckets.len() == 3, "should be 3 buckets");
}

#[test]
fn should_be_empty_if_no_time_until_expiry() {
    let mut rng = TestRng::new();
    let max_deploy_count = 1;
    let max_transfer_count = 1;
    let deploy_config = DeployConfig {
        block_max_deploy_count: max_deploy_count,
        block_max_transfer_count: max_transfer_count,
        block_max_approval_count: max_deploy_count + max_transfer_count,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();

    let secret_key1 = SecretKey::random(&mut rng);
    let ttl = TimeDiff::from_seconds(30);
    let deploy1 = Deploy::random_contract_by_name(
        &mut rng,
        Some(secret_key1),
        None,
        None,
        Some(Timestamp::now()),
        Some(ttl),
    );
    let deploy1_body_hash = *deploy1.header().body_hash();
    deploy_buffer.register_deploy(deploy1);

    let buckets = deploy_buffer.buckets();
    assert!(buckets.len() == 1, "should be 1 buckets");

    let transfer2 = Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
        &mut rng,
        Timestamp::now(),
        ttl,
    );
    assert_ne!(
        &deploy1_body_hash,
        transfer2.header().body_hash(),
        "1 & 2 should have different body hashes"
    );
    deploy_buffer.register_deploy(transfer2);
    let buckets = deploy_buffer.buckets();
    assert!(buckets.len() == 2, "should be 2 buckets");

    let now = Timestamp::now();
    let appendable = deploy_buffer.appendable_block(now, now);
    let count = appendable.deploy_and_transfer_set().len();
    assert!(count == 0, "expected 0 found {}", count);

    // logic should tolerate invalid expiry
    let appendable =
        deploy_buffer.appendable_block(now, now.saturating_sub(TimeDiff::from_millis(1)));
    let count = appendable.deploy_and_transfer_set().len();
    assert!(count == 0, "expected 0 found {}", count);
}

#[test]
fn should_have_diverse_proposable_blocks_with_stocked_buffer() {
    let rng = &mut TestRng::new();
    let max_deploy_count = 50;
    let max_transfer_count = 5;
    let deploy_config = DeployConfig {
        block_max_deploy_count: max_deploy_count,
        block_max_transfer_count: max_transfer_count,
        block_max_approval_count: max_deploy_count + max_transfer_count,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();

    let cap = (max_deploy_count * 100) as usize;

    let secret_keys: Vec<SecretKey> = iter::repeat_with(|| SecretKey::random(rng))
        .take(10)
        .collect();
    let contract_names = ["a", "b", "c", "d", "e"];
    let contract_entry_points = ["foo", "bar"];

    fn ttl(rng: &mut TestRng) -> TimeDiff {
        TimeDiff::from_seconds(rng.gen_range(60..3600))
    }

    let mut last_timestamp = Timestamp::now();
    for i in 0..cap {
        let ttl = ttl(rng);
        let secret_key = Some(
            SecretKey::from_pem(
                secret_keys[rng.gen_range(0..secret_keys.len())]
                    .to_pem()
                    .expect("should pemify"),
            )
            .expect("should un-pemify"),
        );
        let contract_name = Some(contract_names[rng.gen_range(0..contract_names.len())].into());
        let contract_entry_point =
            Some(contract_entry_points[rng.gen_range(0..contract_entry_points.len())].into());
        let deploy = Deploy::random_contract_by_name(
            rng,
            secret_key,
            contract_name,
            contract_entry_point,
            Some(last_timestamp),
            Some(ttl),
        );
        deploy_buffer.register_deploy(deploy);
        assert_eq!(
            deploy_buffer.buffer.len(),
            i + 1,
            "failed to buffer deploy {i}"
        );
        last_timestamp += TimeDiff::from_millis(1);
    }

    for i in 0..max_transfer_count {
        let ttl = ttl(rng);
        deploy_buffer.register_deploy(Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
            rng,
            last_timestamp,
            ttl,
        ));
        assert_eq!(
            deploy_buffer.buffer.len(),
            i as usize + 1 + cap,
            "failed to buffer transfer {i}"
        );
        last_timestamp += TimeDiff::from_millis(1);
    }

    let expected_count = cap + (max_transfer_count as usize);
    assert_container_sizes(&deploy_buffer, expected_count, 0, 0);

    let buckets1 = deploy_buffer.buckets();
    assert!(
        buckets1.len() > 1,
        "should be multiple buckets with this much state"
    );
    let buckets2 = deploy_buffer.buckets();
    assert_eq!(
        buckets1, buckets2,
        "with same state should get same buckets every time"
    );

    // while it is not impossible to get identical appendable blocks over an unchanged buffer
    // using this strategy, it should be very unlikely...the below brute forces a check for this
    let expected_eq_tolerance = 1;
    let mut actual_eq_count = 0;

    let expiry = last_timestamp.saturating_add(TimeDiff::from_seconds(1));
    for _ in 0..10 {
        let appendable1 = deploy_buffer.appendable_block(last_timestamp, expiry);
        let appendable2 = deploy_buffer.appendable_block(last_timestamp, expiry);
        if appendable1 == appendable2 {
            actual_eq_count += 1;
        }
    }
    assert!(
        actual_eq_count <= expected_eq_tolerance,
        "{} matches exceeded tolerance of {}",
        actual_eq_count,
        expected_eq_tolerance
    );
}

/// Event for the mock reactor.
#[derive(Debug)]
enum ReactorEvent {
    DeployBufferAnnouncement(DeployBufferAnnouncement),
    Event(Event),
}

impl From<DeployBufferAnnouncement> for ReactorEvent {
    fn from(req: DeployBufferAnnouncement) -> ReactorEvent {
        ReactorEvent::DeployBufferAnnouncement(req)
    }
}

impl From<Event> for ReactorEvent {
    fn from(req: Event) -> ReactorEvent {
        ReactorEvent::Event(req)
    }
}

struct MockReactor {
    scheduler: &'static Scheduler<ReactorEvent>,
}

impl MockReactor {
    fn new() -> Self {
        MockReactor {
            scheduler: utils::leak(Scheduler::new(QueueKind::weights(), None)),
        }
    }

    async fn expect_deploy_buffer_expire_announcement(
        &self,
        should_be_expired: &HashSet<DeployHash>,
    ) {
        let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
        match reactor_event {
            ReactorEvent::DeployBufferAnnouncement(DeploysExpired(expired)) => {
                let expired_set = HashSet::from_iter(expired);
                assert_eq!(&expired_set, should_be_expired);
            }
            _ => {
                unreachable!();
            }
        };
    }
}

#[tokio::test]
async fn expire_deploys_and_check_announcement() {
    let mut rng = TestRng::new();
    let mut deploy_buffer =
        DeployBuffer::new(DeployConfig::default(), Config::default(), &Registry::new()).unwrap();

    let reactor = MockReactor::new();
    let event_queue_handle = EventQueueHandle::without_shutdown(reactor.scheduler);
    let effect_builder = EffectBuilder::new(event_queue_handle);

    // generate and register some already expired deploys
    let ttl = TimeDiff::from_seconds(rng.gen_range(30..300));
    let past_timestamp = Timestamp::now()
        .saturating_sub(ttl)
        .saturating_sub(TimeDiff::from_seconds(5));

    let num_deploys: usize = rng.gen_range(5..50);
    let expired_deploys = create_valid_deploys(
        &mut rng,
        num_deploys,
        DeployType::Transfer,
        Some(past_timestamp),
        Some(ttl),
    );
    expired_deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, expired_deploys.len(), 0, 0);

    // include the last expired deploy in a block and register it
    let block = Block::random_with_deploys(&mut rng, expired_deploys.last());
    deploy_buffer.register_block(&block);
    assert_container_sizes(&deploy_buffer, expired_deploys.len(), 1, 0);

    // generate and register some valid deploys
    let deploys = create_valid_deploys(&mut rng, num_deploys, DeployType::Transfer, None, None);
    deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, deploys.len() + expired_deploys.len(), 1, 0);

    // expire deploys and check that they were announced as expired
    let mut effects = deploy_buffer.expire(effect_builder);
    tokio::spawn(effects.remove(0)).await.unwrap();

    // the deploys which should be announced as expired are all the expired ones not in a block,
    // i.e. all but the last one of `expired_deploys`
    let expired_deploy_hashes: HashSet<_> = expired_deploys
        .iter()
        .take(expired_deploys.len() - 1)
        .map(|deploy| *deploy.hash())
        .collect();
    reactor
        .expect_deploy_buffer_expire_announcement(&expired_deploy_hashes)
        .await;

    // the valid deploys should still be in the buffer
    assert_container_sizes(&deploy_buffer, deploys.len(), 0, 0);
}

fn register_random_deploys_unique_hashes(
    deploy_buffer: &mut DeployBuffer,
    num_deploys: usize,
    rng: &mut TestRng,
) {
    let deploys = std::iter::repeat_with(|| {
        let name = format!("{}", rng.gen::<u64>());
        let call = format!("{}", rng.gen::<u64>());
        Deploy::random_contract_by_name(
            rng,
            None,
            Some(name),
            Some(call),
            Some(Timestamp::now()), // different timestamp
            None,
        )
    })
    .take(num_deploys);
    for deploy in deploys {
        deploy_buffer.register_deploy(deploy);
    }
}

fn register_random_deploys_same_hash(
    deploy_buffer: &mut DeployBuffer,
    num_deploys: usize,
    rng: &mut TestRng,
) {
    let deploys = std::iter::repeat_with(|| {
        let name = "test".to_owned();
        let call = "test".to_owned();
        Deploy::random_contract_by_name(
            rng,
            None,
            Some(name),
            Some(call),
            Some(Timestamp::now()), // different timestamp
            None,
        )
    })
    .take(num_deploys);
    for deploy in deploys {
        deploy_buffer.register_deploy(deploy);
    }
}

#[test]
fn test_buckets_single_hash() {
    let mut rng = TestRng::new();
    let deploy_config = DeployConfig {
        block_max_transfer_count: 1000,
        block_max_deploy_count: 100,
        block_max_approval_count: 1100,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();

    register_random_deploys_same_hash(&mut deploy_buffer, 64000, &mut rng);

    let _block = deploy_buffer.appendable_block(
        Timestamp::now(),
        Timestamp::now() + TimeDiff::from_millis(16384 / 6),
    );
}

#[test]
fn test_buckets_unique_hashes() {
    let mut rng = TestRng::new();
    let deploy_config = DeployConfig {
        block_max_transfer_count: 1000,
        block_max_deploy_count: 100,
        block_max_approval_count: 1100,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();

    register_random_deploys_unique_hashes(&mut deploy_buffer, 64000, &mut rng);

    let _block = deploy_buffer.appendable_block(
        Timestamp::now(),
        Timestamp::now() + TimeDiff::from_millis(16384 / 6),
    );
}

#[test]
fn test_buckets_mixed_load() {
    let mut rng = TestRng::new();
    let deploy_config = DeployConfig {
        block_max_transfer_count: 1000,
        block_max_deploy_count: 100,
        block_max_approval_count: 1100,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(deploy_config, Config::default(), &Registry::new()).unwrap();

    register_random_deploys_unique_hashes(&mut deploy_buffer, 60000, &mut rng);
    register_random_deploys_same_hash(&mut deploy_buffer, 4000, &mut rng);

    let _block = deploy_buffer.appendable_block(
        Timestamp::now(),
        Timestamp::now() + TimeDiff::from_millis(16384 / 6),
    );
}
