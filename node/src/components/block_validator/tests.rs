use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

use derive_more::From;
use itertools::Itertools;
use rand::Rng;

use casper_types::{
    bytesrepr::Bytes, runtime_args, system::standard_payment::ARG_AMOUNT, testing::TestRng,
    Chainspec, ChainspecRawBytes, Deploy, DeployHash, ExecutableDeployItem, RuntimeArgs, SecretKey,
    TimeDiff, Transaction, TransactionHash, U512,
};

use crate::{
    components::{
        consensus::BlockContext,
        fetcher::{self, FetchItem},
    },
    effect::announcements::FatalAnnouncement,
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::{BlockPayload, ValidatorMatrix},
    utils::{self, Loadable},
};

use super::*;

#[derive(Debug, From)]
enum ReactorEvent {
    #[from]
    BlockValidator(Event),
    #[from]
    TransactionFetcher(FetcherRequest<Transaction>),
    #[from]
    FinalitySigFetcher(FetcherRequest<FinalitySignature>),
    #[from]
    Storage(StorageRequest),
    #[from]
    FatalAnnouncement(FatalAnnouncement),
}

impl From<BlockValidationRequest> for ReactorEvent {
    fn from(req: BlockValidationRequest) -> ReactorEvent {
        ReactorEvent::BlockValidator(req.into())
    }
}

struct MockReactor {
    scheduler: &'static Scheduler<ReactorEvent>,
    validator_matrix: ValidatorMatrix,
}

impl MockReactor {
    fn new(rng: &mut TestRng) -> Self {
        MockReactor {
            scheduler: utils::leak(Scheduler::new(QueueKind::weights(), None)),
            validator_matrix: ValidatorMatrix::new_with_validator(Arc::new(SecretKey::random(rng))),
        }
    }

    async fn expect_block_validator_event(&self) -> Event {
        let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
        if let ReactorEvent::BlockValidator(event) = reactor_event {
            event
        } else {
            panic!("unexpected event: {:?}", reactor_event);
        }
    }

    async fn expect_fetch_deploys(
        &self,
        mut deploys_to_fetch: Vec<Deploy>,
        mut deploys_to_not_fetch: HashSet<DeployHash>,
    ) {
        while !deploys_to_fetch.is_empty() || !deploys_to_not_fetch.is_empty() {
            let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
            if let ReactorEvent::TransactionFetcher(FetcherRequest {
                id,
                peer,
                validation_metadata: _,
                responder,
            }) = reactor_event
            {
                if let Some((position, _)) = deploys_to_fetch
                    .iter()
                    .find_position(|&deploy| Transaction::Deploy(deploy.clone()).fetch_id() == id)
                {
                    let deploy = deploys_to_fetch.remove(position);
                    let response = FetchedData::FromPeer {
                        item: Box::new(Transaction::Deploy(deploy)),
                        peer,
                    };
                    responder.respond(Ok(response)).await;
                } else if deploys_to_not_fetch.remove(&match id.transaction_hash() {
                    TransactionHash::Deploy(deploy_hash) => deploy_hash,
                    TransactionHash::V1(_) => unreachable!("only fetching deploys for now"),
                }) {
                    responder
                        .respond(Err(fetcher::Error::Absent {
                            id: Box::new(id),
                            peer,
                        }))
                        .await
                } else {
                    panic!("unexpected fetch request: {}", id);
                }
            } else {
                panic!("unexpected event: {:?}", reactor_event);
            }
        }
    }
}

pub(super) fn new_proposed_block(
    timestamp: Timestamp,
    transfer: Vec<TransactionHashWithApprovals>,
    staking: Vec<TransactionHashWithApprovals>,
    install_upgrade: Vec<TransactionHashWithApprovals>,
    standard: Vec<TransactionHashWithApprovals>,
) -> ProposedBlock<ClContext> {
    // Accusations and ancestors are empty, and the random bit is always true:
    // These values are not checked by the block validator.
    let block_context = BlockContext::new(timestamp, vec![]);
    let block_payload = BlockPayload::new(
        transfer,
        staking,
        install_upgrade,
        standard,
        vec![],
        Default::default(),
        true,
    );
    ProposedBlock::new(Arc::new(block_payload), block_context)
}

pub(super) fn new_deploy(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Deploy {
    let secret_key = SecretKey::random(rng);
    let chain_name = "chain".to_string();
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: runtime_args! { ARG_AMOUNT => U512::from(1) },
    };
    let session = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: RuntimeArgs::new(),
    };
    let dependencies = vec![];
    let gas_price = 1;

    Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        None,
    )
}

pub(super) fn new_transfer(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Deploy {
    let secret_key = SecretKey::random(rng);
    let chain_name = "chain".to_string();
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: runtime_args! { ARG_AMOUNT => U512::from(1) },
    };
    let session = ExecutableDeployItem::Transfer {
        args: RuntimeArgs::new(),
    };
    let dependencies = vec![];
    let gas_price = 1;

    Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        None,
    )
}

/// Validates a block using a `BlockValidator` component, and returns the result.
async fn validate_block(
    rng: &mut TestRng,
    timestamp: Timestamp,
    deploys: Vec<Deploy>,
    transfers: Vec<Deploy>,
) -> bool {
    // Assemble the block to be validated.
    let transfers_for_block = transfers
        .iter()
        .map(|deploy| TransactionHashWithApprovals::from(&Transaction::Deploy(deploy.clone())))
        .collect_vec();
    let standard_for_block = deploys
        .iter()
        .map(|deploy| TransactionHashWithApprovals::from(&Transaction::Deploy(deploy.clone())))
        .collect_vec();
    let proposed_block = new_proposed_block(
        timestamp,
        transfers_for_block,
        vec![],
        vec![],
        standard_for_block,
    );

    // Create the reactor and component.
    let reactor = MockReactor::new(rng);
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));
    let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut block_validator = BlockValidator::new(
        Arc::new(chainspec),
        reactor.validator_matrix.clone(),
        Config::default(),
    );

    // Pass the block to the component. This future will eventually resolve to the result, i.e.
    // whether the block is valid or not.
    let bob_node_id = NodeId::random(rng);
    let block_height = rng.gen_range(0..1000);
    let validation_result = tokio::spawn(effect_builder.validate_block(
        bob_node_id,
        block_height,
        proposed_block.clone(),
    ));
    let event = reactor.expect_block_validator_event().await;
    let effects = block_validator.handle_event(effect_builder, rng, event);

    // If validity could already be determined, the effect will be the validation response.
    if block_validator
        .validation_states
        .values()
        .all(BlockValidationState::completed)
    {
        assert_eq!(1, effects.len());
        for effect in effects {
            tokio::spawn(effect).await.unwrap(); // Response.
        }
        return validation_result.await.unwrap();
    }

    // Otherwise the effects must be requests to fetch the block's deploys.
    let fetch_results: Vec<_> = effects.into_iter().map(tokio::spawn).collect();

    // We make our mock reactor answer with the expected deploys and transfers:
    reactor
        .expect_fetch_deploys(
            deploys.into_iter().chain(transfers).collect(),
            HashSet::new(),
        )
        .await;

    // The resulting `FetchResult`s are passed back into the component. When any deploy turns out
    // to be invalid, or once all of them have been validated, the component will respond.
    let mut effects = Effects::new();
    for fetch_result in fetch_results {
        let events = fetch_result.await.unwrap();
        assert_eq!(1, events.len());
        effects.extend(events.into_iter().flat_map(|found_deploy| {
            block_validator.handle_event(effect_builder, rng, found_deploy)
        }));
    }

    // We expect exactly one effect: the validation response. This will resolve the result.
    assert_eq!(1, effects.len());
    for effect in effects {
        tokio::spawn(effect).await.unwrap(); // Response.
    }
    validation_result.await.unwrap()
}

/// Verifies that a block without any deploys or transfers is valid.
#[tokio::test]
async fn empty_block() {
    assert!(validate_block(&mut TestRng::new(), 1000.into(), vec![], vec![]).await);
}

/// Verifies that the block validator checks deploy and transfer timestamps and ttl.
#[tokio::test]
async fn ttl() {
    // The ttl is 200 ms, and our deploys and transfers have timestamps 900 and 1000. So the block
    // timestamp must be at least 1000 and at most 1100.
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let deploys = vec![
        new_deploy(&mut rng, 1000.into(), ttl),
        new_deploy(&mut rng, 900.into(), ttl),
    ];
    let transfers = vec![
        new_transfer(&mut rng, 1000.into(), ttl),
        new_transfer(&mut rng, 900.into(), ttl),
    ];

    // Both 1000 and 1100 are timestamps compatible with the deploys and transfers.
    assert!(validate_block(&mut rng, 1000.into(), deploys.clone(), transfers.clone()).await);
    assert!(validate_block(&mut rng, 1100.into(), deploys.clone(), transfers.clone()).await);

    // A block with timestamp 999 can't contain a transfer or deploy with timestamp 1000.
    assert!(!validate_block(&mut rng, 999.into(), deploys.clone(), vec![]).await);
    assert!(!validate_block(&mut rng, 999.into(), vec![], transfers.clone()).await);
    assert!(!validate_block(&mut rng, 999.into(), deploys.clone(), transfers.clone()).await);

    // At time 1101, the deploy and transfer from time 900 have expired.
    assert!(!validate_block(&mut rng, 1101.into(), deploys.clone(), vec![]).await);
    assert!(!validate_block(&mut rng, 1101.into(), vec![], transfers.clone()).await);
    assert!(!validate_block(&mut rng, 1101.into(), deploys, transfers).await);
}

/// Verifies that a block is invalid if it contains a transfer in the `deploy_hashes` or a
/// non-transfer deploy in the `transfer_hashes`, or if it contains a replay.
#[tokio::test]
async fn transfer_deploy_mixup_and_replay() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy1 = new_deploy(&mut rng, timestamp, ttl);
    let deploy2 = new_deploy(&mut rng, timestamp, ttl);
    let transfer1 = new_transfer(&mut rng, timestamp, ttl);
    let transfer2 = new_transfer(&mut rng, timestamp, ttl);

    // First we make sure that our transfers and deploys would normally be valid.
    let deploys = vec![deploy1.clone(), deploy2.clone()];
    let transfers = vec![transfer1.clone(), transfer2.clone()];
    assert!(validate_block(&mut rng, timestamp, deploys, transfers).await);

    // Now we hide a transfer in the deploys section. This should be invalid.
    let deploys = vec![deploy1.clone(), deploy2.clone(), transfer2.clone()];
    let transfers = vec![transfer1.clone()];
    assert!(!validate_block(&mut rng, timestamp, deploys, transfers).await);

    // A regular deploy in the transfers section is also invalid.
    let deploys = vec![deploy2.clone()];
    let transfers = vec![transfer1.clone(), deploy1.clone(), transfer2.clone()];
    assert!(!validate_block(&mut rng, timestamp, deploys, transfers).await);

    // Each deploy must be unique
    let deploys = vec![deploy1.clone(), deploy2.clone(), deploy1.clone()];
    let transfers = vec![transfer1.clone(), transfer2.clone()];
    assert!(!validate_block(&mut rng, timestamp, deploys, transfers).await);

    // And each transfer must be unique, too.
    let deploys = vec![deploy1.clone(), deploy2.clone()];
    let transfers = vec![transfer1.clone(), transfer2.clone(), transfer2.clone()];
    assert!(!validate_block(&mut rng, timestamp, deploys, transfers).await);
}

/// Verifies that the block validator fetches from multiple peers.
#[tokio::test]
async fn should_fetch_from_multiple_peers() {
    let _ = crate::logging::init();
    tokio::time::timeout(Duration::from_secs(5), async move {
        let peer_count = 3;
        let mut rng = TestRng::new();
        let ttl = TimeDiff::from_seconds(200);
        let deploys = (0..peer_count)
            .map(|i| new_deploy(&mut rng, (900 + i).into(), ttl))
            .collect_vec();
        let transfers = (0..peer_count)
            .map(|i| new_transfer(&mut rng, (1000 + i).into(), ttl))
            .collect_vec();

        // Assemble the block to be validated.
        let transfers_for_block = transfers
            .iter()
            .map(|deploy| TransactionHashWithApprovals::from(&Transaction::Deploy(deploy.clone())))
            .collect_vec();
        let standard_for_block = deploys
            .iter()
            .map(|deploy| TransactionHashWithApprovals::from(&Transaction::Deploy(deploy.clone())))
            .collect_vec();
        let proposed_block = new_proposed_block(
            1100.into(),
            transfers_for_block,
            vec![],
            vec![],
            standard_for_block,
        );

        // Create the reactor and component.
        let reactor = MockReactor::new(&mut rng);
        let effect_builder =
            EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));
        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        let mut block_validator = BlockValidator::new(
            Arc::new(chainspec),
            reactor.validator_matrix.clone(),
            Config::default(),
        );

        // Have a validation request for each one of the peers. These futures will eventually all
        // resolve to the same result, i.e. whether the block is valid or not.
        let validation_results = (0..peer_count)
            .map(|_| {
                let node_id = NodeId::random(&mut rng);
                let block_height = rng.gen_range(0..1000);
                tokio::spawn(effect_builder.validate_block(
                    node_id,
                    block_height,
                    proposed_block.clone(),
                ))
            })
            .collect_vec();

        let mut fetch_effects = VecDeque::new();
        for index in 0..peer_count {
            let event = reactor.expect_block_validator_event().await;
            let effects = block_validator.handle_event(effect_builder, &mut rng, event);
            if index == 0 {
                assert_eq!(effects.len(), 6);
                fetch_effects.extend(effects);
            } else {
                assert!(effects.is_empty());
            }
        }

        // The effects are requests to fetch the block's deploys.  There are six fetch requests, all
        // using the first peer.
        let fetch_results = fetch_effects.drain(..).map(tokio::spawn).collect_vec();

        // Provide the first deploy and transfer on first asking.
        let deploys_to_fetch = vec![deploys[0].clone(), transfers[0].clone()];
        let deploys_to_not_fetch = vec![
            *deploys[1].hash(),
            *deploys[2].hash(),
            *transfers[1].hash(),
            *transfers[2].hash(),
        ]
        .into_iter()
        .collect();
        reactor
            .expect_fetch_deploys(deploys_to_fetch, deploys_to_not_fetch)
            .await;

        let mut missing = vec![];
        for fetch_result in fetch_results {
            let mut events = fetch_result.await.unwrap();
            assert_eq!(1, events.len());
            // The event should be `DeployFetched`.
            let event = events.pop().unwrap();
            // New fetch requests will be made using a different peer for all deploys not already
            // registered as fetched.
            let effects = block_validator.handle_event(effect_builder, &mut rng, event);
            if !effects.is_empty() {
                assert!(missing.is_empty());
                missing = block_validator
                    .validation_states
                    .values()
                    .next()
                    .unwrap()
                    .missing_hashes();
            }
            fetch_effects.extend(effects);
        }

        // Handle the second set of fetch requests now.
        let fetch_results = fetch_effects.drain(..).map(tokio::spawn).collect_vec();

        // Provide the first and second deploys and transfers which haven't already been fetched on
        // second asking.
        let deploys_to_fetch = vec![&deploys[0], &deploys[1], &transfers[0], &transfers[1]]
            .into_iter()
            .filter(|deploy| missing.contains(deploy.hash()))
            .cloned()
            .collect();
        let deploys_to_not_fetch = vec![*deploys[2].hash(), *transfers[2].hash()]
            .into_iter()
            .filter(|deploy_hash| missing.contains(deploy_hash))
            .collect();
        reactor
            .expect_fetch_deploys(deploys_to_fetch, deploys_to_not_fetch)
            .await;

        missing.clear();
        for fetch_result in fetch_results {
            let mut events = fetch_result.await.unwrap();
            assert_eq!(1, events.len());
            // The event should be `DeployFetched`.
            let event = events.pop().unwrap();
            // New fetch requests will be made using a different peer for all deploys not already
            // registered as fetched.
            let effects = block_validator.handle_event(effect_builder, &mut rng, event);
            if !effects.is_empty() {
                assert!(missing.is_empty());
                missing = block_validator
                    .validation_states
                    .values()
                    .next()
                    .unwrap()
                    .missing_hashes();
            }
            fetch_effects.extend(effects);
        }

        // Handle the final set of fetch requests now.
        let fetch_results = fetch_effects.into_iter().map(tokio::spawn).collect_vec();

        // Provide all deploys and transfers not already fetched on third asking.
        let deploys_to_fetch = deploys
            .iter()
            .chain(transfers.iter())
            .filter(|deploy| missing.contains(deploy.hash()))
            .cloned()
            .collect();
        reactor
            .expect_fetch_deploys(deploys_to_fetch, HashSet::new())
            .await;

        let mut effects = Effects::new();
        for fetch_result in fetch_results {
            let mut events = fetch_result.await.unwrap();
            assert_eq!(1, events.len());
            // The event should be `DeployFetched`.
            let event = events.pop().unwrap();
            // Once the block is deemed valid (i.e. when the final missing deploy is successfully
            // fetched) the effects will be three validation responses.
            effects.extend(block_validator.handle_event(effect_builder, &mut rng, event));
            assert!(effects.is_empty() || effects.len() == peer_count as usize);
        }

        for effect in effects {
            tokio::spawn(effect).await.unwrap();
        }

        for validation_result in validation_results {
            assert!(validation_result.await.unwrap());
        }
    })
    .await
    .expect("should not hang");
}
