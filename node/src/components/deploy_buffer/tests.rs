use prometheus::Registry;
use rand::Rng;

use casper_types::{testing::TestRng, EraId, TestBlockBuilder, TimeDiff};

use super::*;
use crate::{
    effect::announcements::DeployBufferAnnouncement::{self, DeploysExpired},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::FinalizedBlock,
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

fn create_invalid_deploys(rng: &mut TestRng, size: usize) -> Vec<Deploy> {
    let mut deploys = create_valid_deploys(rng, size, DeployType::Random, None, None);

    for deploy in deploys.iter_mut() {
        deploy.invalidate();
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
    assert_eq!(deploy_buffer.buffer.len(), expected_buffer);
    assert_eq!(deploy_buffer.dead.len(), expected_dead);
    assert_eq!(
        deploy_buffer
            .hold
            .values()
            .map(|deploys| deploys.len())
            .sum::<usize>(),
        expected_held
    );
    assert_eq!(
        deploy_buffer.metrics.total_deploys.get(),
        expected_buffer as i64
    );
    assert_eq!(
        deploy_buffer.metrics.held_deploys.get(),
        expected_held as i64
    );
    assert_eq!(
        deploy_buffer.metrics.dead_deploys.get(),
        expected_dead as i64
    );
}

#[test]
fn register_deploy_and_check_size() {
    let mut rng = TestRng::new();
    let mut deploy_buffer = DeployBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    // Try to register valid deploys
    let num_valid_deploys: usize = rng.gen_range(50..500);
    let valid_deploys =
        create_valid_deploys(&mut rng, num_valid_deploys, DeployType::Random, None, None);
    valid_deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, valid_deploys.len(), 0, 0);

    // Try to register invalid deploys
    let num_invalid_deploys: usize = rng.gen_range(10..100);
    let invalid_deploys = create_invalid_deploys(&mut rng, num_invalid_deploys);
    invalid_deploys
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
    let mut deploy_buffer = DeployBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    let deploys = create_valid_deploys(&mut rng, 10, DeployType::Random, None, None);
    let era_id = EraId::new(rng.gen_range(0..6));
    let height = era_id.value() * 10 + rng.gen_range(0..10);
    let is_switch = rng.gen_bool(0.1);
    let block = TestBlockBuilder::new()
        .era(era_id)
        .height(height)
        .switch_block(is_switch)
        .deploys(deploys.iter())
        .build(&mut rng);

    deploy_buffer.register_block(&block);
    assert_container_sizes(&deploy_buffer, deploys.len(), deploys.len(), 0);
}

#[test]
fn register_finalized_block_with_valid_deploys() {
    let mut rng = TestRng::new();
    let mut deploy_buffer = DeployBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    let deploys = create_valid_deploys(&mut rng, 10, DeployType::Random, None, None);
    let block = FinalizedBlock::random(&mut rng, deploys.iter());

    deploy_buffer.register_block_finalized(&block);
    assert_container_sizes(&deploy_buffer, deploys.len(), deploys.len(), 0);
}

#[test]
fn get_proposable_deploys() {
    let mut rng = TestRng::new();
    let mut deploy_buffer = DeployBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

    // populate deploy buffer with some deploys
    let deploys = create_valid_deploys(&mut rng, 50, DeployType::Random, None, None);
    deploys
        .iter()
        .for_each(|deploy| deploy_buffer.register_deploy(deploy.clone()));
    assert_container_sizes(&deploy_buffer, deploys.len(), 0, 0);

    // Create a block with some deploys and register it with the deploy_buffer
    let block_deploys = create_valid_deploys(&mut rng, 10, DeployType::Random, None, None);
    let block = FinalizedBlock::random(&mut rng, block_deploys.iter());
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

    // Get an appendable block. This should put the deploys on hold.
    let appendable_block = deploy_buffer.appendable_block(Timestamp::now());
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
    let transaction_config = TransactionConfig {
        block_max_deploy_count: 10,
        block_max_native_count: 200,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut deploy_buffer,
        DeployType::Transfer,
        transaction_config.block_max_native_count as usize,
    );
}

#[test]
fn get_appendable_block_with_standard_deploys() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_deploy_count: 10,
        block_max_native_count: 200,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut deploy_buffer,
        DeployType::Standard,
        transaction_config.block_max_deploy_count as usize,
    );
}

#[test]
fn get_appendable_block_with_random_deploys() {
    let mut rng = TestRng::new();
    let transaction_config = TransactionConfig {
        block_max_deploy_count: 10,
        block_max_native_count: 200,
        block_max_approval_count: 210,
        ..Default::default()
    };
    let mut deploy_buffer =
        DeployBuffer::new(transaction_config, Config::default(), &Registry::new()).unwrap();
    get_appendable_block(
        &mut rng,
        &mut deploy_buffer,
        DeployType::Random,
        transaction_config.block_max_native_count as usize,
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

    // now check how many transfers were added in the block; should not exceed the config limits.
    let appendable_block = deploy_buffer.appendable_block(Timestamp::now());
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
    let mut deploy_buffer = DeployBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

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
    let era = rng.gen_range(0..6);
    let height = era * 10 + rng.gen_range(0..10);
    let is_switch = rng.gen_bool(0.1);

    let block = TestBlockBuilder::new()
        .era(era)
        .height(height)
        .switch_block(is_switch)
        .deploys(block_deploys.iter())
        .build(&mut rng);

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

    // get an appendable block. This should put the deploys on hold.
    let appendable_block = deploy_buffer.appendable_block(Timestamp::now());
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
    let mut deploy_buffer = DeployBuffer::new(
        TransactionConfig::default(),
        Config::default(),
        &Registry::new(),
    )
    .unwrap();

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
    let era = rng.gen_range(0..6);
    let block = TestBlockBuilder::new()
        .era(era)
        .height(era * 10 + rng.gen_range(0..10))
        .deploys(expired_deploys.last())
        .build(&mut rng);

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
