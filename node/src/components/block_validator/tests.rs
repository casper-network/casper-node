use std::{collections::VecDeque, sync::Arc, time::Duration};

use derive_more::From;
use itertools::Itertools;
use rand::Rng;

use casper_types::{
    bytesrepr::Bytes, runtime_args, system::standard_payment::ARG_AMOUNT, testing::TestRng, Block,
    BlockSignatures, BlockSignaturesV2, Chainspec, ChainspecRawBytes, Deploy, ExecutableDeployItem,
    FinalitySignatureV2, RuntimeArgs, SecretKey, TestBlockBuilder, TimeDiff, Transaction, U512,
};

use crate::{
    components::{
        consensus::BlockContext,
        fetcher::{self, FetchItem},
    },
    effect::{announcements::FatalAnnouncement, requests::StorageRequest},
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
    fn new<I: IntoIterator<Item = PublicKey>>(
        our_secret_key: Arc<SecretKey>,
        public_keys: I,
    ) -> Self {
        MockReactor {
            scheduler: utils::leak(Scheduler::new(QueueKind::weights(), None)),
            validator_matrix: ValidatorMatrix::new_with_validators(our_secret_key, public_keys),
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

    async fn handle_requests(&self, context: &ValidationContext) {
        while let Ok(((_ancestor, event), _)) =
            tokio::time::timeout(Duration::from_millis(100), self.scheduler.pop()).await
        {
            match event {
                ReactorEvent::TransactionFetcher(FetcherRequest {
                    id,
                    peer,
                    validation_metadata: _,
                    responder,
                }) => {
                    if let Some(deploy) = context.get_deploy(id) {
                        let response = FetchedData::FromPeer {
                            item: Box::new(Transaction::Deploy(deploy)),
                            peer,
                        };
                        responder.respond(Ok(response)).await;
                    } else {
                        responder
                            .respond(Err(fetcher::Error::Absent {
                                id: Box::new(id),
                                peer,
                            }))
                            .await;
                    }
                }
                ReactorEvent::Storage(StorageRequest::GetBlockAndMetadataByHeight {
                    block_height,
                    only_from_available_block_range: _,
                    responder,
                }) => {
                    let maybe_block = context.get_block_with_metadata(block_height);
                    responder.respond(maybe_block).await;
                }
                ReactorEvent::FinalitySigFetcher(FetcherRequest {
                    id,
                    peer,
                    validation_metadata: _,
                    responder,
                }) => {
                    if let Some(signature) = context.get_signature(&id) {
                        let response = FetchedData::FromPeer {
                            item: Box::new(signature),
                            peer,
                        };
                        responder.respond(Ok(response)).await;
                    } else {
                        responder
                            .respond(Err(fetcher::Error::Absent {
                                id: Box::new(id),
                                peer,
                            }))
                            .await;
                    }
                }
                reactor_event => {
                    panic!("unexpected event: {:?}", reactor_event);
                }
            };
        }
    }
}

pub(super) fn new_proposed_block_with_cited_signatures(
    timestamp: Timestamp,
    transfer: Vec<TransactionHashWithApprovals>,
    staking: Vec<TransactionHashWithApprovals>,
    install_upgrade: Vec<TransactionHashWithApprovals>,
    standard: Vec<TransactionHashWithApprovals>,
    cited_signatures: RewardedSignatures,
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
        cited_signatures,
        true,
    );
    ProposedBlock::new(Arc::new(block_payload), block_context)
}

pub(super) fn new_proposed_block(
    timestamp: Timestamp,
    transfer: Vec<TransactionHashWithApprovals>,
    staking: Vec<TransactionHashWithApprovals>,
    install_upgrade: Vec<TransactionHashWithApprovals>,
    standard: Vec<TransactionHashWithApprovals>,
) -> ProposedBlock<ClContext> {
    new_proposed_block_with_cited_signatures(
        timestamp,
        transfer,
        staking,
        install_upgrade,
        standard,
        Default::default(),
    )
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

type SecretKeys = BTreeMap<PublicKey, Arc<SecretKey>>;

struct ValidationContext {
    chainspec: Arc<Chainspec>,
    // Validators
    secret_keys: SecretKeys,
    // map of height → block
    past_blocks: HashMap<u64, Block>,
    // blocks that will be "stored" during validation
    delayed_blocks: HashMap<u64, Block>,
    deploys: HashMap<TransactionId, Deploy>,
    transfers: HashMap<TransactionId, Deploy>,
    // map of block height → signatures for the block
    signatures: HashMap<u64, HashMap<PublicKey, FinalitySignatureV2>>,
    // map of signatures that aren't stored, but are fetchable
    fetchable_signatures: HashMap<FinalitySignatureId, FinalitySignature>,

    // fields defining the proposed block that will be validated
    deploys_to_include: Vec<TransactionHashWithApprovals>,
    transfers_to_include: Vec<TransactionHashWithApprovals>,
    signatures_to_include: HashMap<u64, BTreeSet<PublicKey>>,
    proposed_block_height: Option<u64>,
}

impl ValidationContext {
    fn new() -> Self {
        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        Self {
            chainspec: Arc::new(chainspec),
            secret_keys: BTreeMap::new(),
            past_blocks: HashMap::new(),
            delayed_blocks: HashMap::new(),
            deploys: HashMap::new(),
            transfers: HashMap::new(),
            fetchable_signatures: HashMap::new(),
            signatures: HashMap::new(),
            deploys_to_include: vec![],
            transfers_to_include: vec![],
            signatures_to_include: HashMap::new(),
            proposed_block_height: None,
        }
    }

    fn with_num_validators(mut self, rng: &mut TestRng, num_validators: usize) -> Self {
        for _ in 0..num_validators {
            let validator_key = Arc::new(SecretKey::random(rng));
            self.secret_keys
                .insert(PublicKey::from(&*validator_key), validator_key.clone());
        }
        self
    }

    fn get_validators(&self) -> Vec<PublicKey> {
        self.secret_keys.keys().cloned().collect()
    }

    fn with_past_blocks(
        mut self,
        rng: &mut TestRng,
        min_height: u64,
        max_height: u64,
        era: EraId,
    ) -> Self {
        self.past_blocks
            .extend((min_height..=max_height).into_iter().map(|height| {
                let block = TestBlockBuilder::new().height(height).era(era).build(rng);
                (height, block.into())
            }));
        self.proposed_block_height = self
            .proposed_block_height
            .map(|height| height.max(max_height + 1))
            .or(Some(max_height + 1));
        self
    }

    fn with_delayed_blocks(
        mut self,
        rng: &mut TestRng,
        min_height: u64,
        max_height: u64,
        era: EraId,
    ) -> Self {
        self.delayed_blocks
            .extend((min_height..=max_height).into_iter().map(|height| {
                let block = TestBlockBuilder::new().height(height).era(era).build(rng);
                (height, block.into())
            }));
        self.proposed_block_height = self
            .proposed_block_height
            .map(|height| height.max(max_height + 1))
            .or(Some(max_height + 1));
        self
    }

    fn get_delayed_blocks(&mut self) -> Vec<u64> {
        let heights = self.delayed_blocks.keys().cloned().collect();
        self.past_blocks
            .extend(std::mem::take(&mut self.delayed_blocks));
        heights
    }

    fn with_signatures_for_block<'a, I: IntoIterator<Item = &'a PublicKey>>(
        mut self,
        min_height: u64,
        max_height: u64,
        validators: I,
    ) -> Self {
        for validator in validators {
            for height in min_height..=max_height {
                let block = self
                    .past_blocks
                    .get(&height)
                    .or_else(|| self.delayed_blocks.get(&height))
                    .expect("should have block");
                let secret_key = self
                    .secret_keys
                    .get(validator)
                    .expect("should have validator");
                let signature = FinalitySignatureV2::create(
                    *block.hash(),
                    block.height(),
                    block.era_id(),
                    self.chainspec.name_hash(),
                    secret_key,
                );
                self.signatures
                    .entry(height)
                    .or_default()
                    .insert(validator.clone(), signature);
            }
        }
        self
    }

    fn with_fetchable_signatures<'a, I: IntoIterator<Item = &'a PublicKey>>(
        mut self,
        min_height: u64,
        max_height: u64,
        validators: I,
    ) -> Self {
        for validator in validators {
            for height in min_height..=max_height {
                let block = self.past_blocks.get(&height).expect("should have block");
                let secret_key = self
                    .secret_keys
                    .get(validator)
                    .expect("should have validator");
                let signature = FinalitySignature::V2(FinalitySignatureV2::create(
                    *block.hash(),
                    block.height(),
                    block.era_id(),
                    self.chainspec.name_hash(),
                    secret_key,
                ));
                self.fetchable_signatures
                    .insert(*signature.fetch_id(), signature);
            }
        }
        self
    }

    fn include_signatures<'a, I: IntoIterator<Item = &'a PublicKey>>(
        mut self,
        min_height: u64,
        max_height: u64,
        validators: I,
    ) -> Self {
        for validator in validators {
            for height in min_height..=max_height {
                self.signatures_to_include
                    .entry(height)
                    .or_default()
                    .insert(validator.clone());
            }
        }
        self
    }

    fn with_deploys(mut self, deploys: Vec<Deploy>) -> Self {
        self.deploys.extend(
            deploys
                .into_iter()
                .map(|deploy| (Transaction::Deploy(deploy.clone()).fetch_id(), deploy)),
        );
        self
    }

    fn with_transfers(mut self, transfers: Vec<Deploy>) -> Self {
        self.transfers.extend(
            transfers
                .into_iter()
                .map(|deploy| (Transaction::Deploy(deploy.clone()).fetch_id(), deploy)),
        );
        self
    }

    fn include_all_deploys(mut self) -> Self {
        self.deploys_to_include
            .extend(self.deploys.values().map(|deploy| {
                TransactionHashWithApprovals::from(&Transaction::Deploy(deploy.clone()))
            }));
        self
    }

    fn include_all_transfers(mut self) -> Self {
        self.transfers_to_include
            .extend(self.transfers.values().map(|deploy| {
                TransactionHashWithApprovals::from(&Transaction::Deploy(deploy.clone()))
            }));
        self
    }

    fn include_deploys<I: IntoIterator<Item = TransactionHashWithApprovals>>(
        mut self,
        deploys: I,
    ) -> Self {
        self.deploys_to_include.extend(deploys);
        self
    }

    fn include_transfers<I: IntoIterator<Item = TransactionHashWithApprovals>>(
        mut self,
        transfers: I,
    ) -> Self {
        self.transfers_to_include.extend(transfers);
        self
    }

    fn get_deploy(&self, id: TransactionId) -> Option<Deploy> {
        self.deploys
            .get(&id)
            .cloned()
            .or_else(|| self.transfers.get(&id).cloned())
    }

    fn get_signature(&self, id: &FinalitySignatureId) -> Option<FinalitySignature> {
        self.fetchable_signatures.get(id).cloned()
    }

    fn get_block_with_metadata(&self, block_height: u64) -> Option<BlockWithMetadata> {
        self.past_blocks.get(&block_height).map(|block| {
            let empty_hashmap = HashMap::new();
            let signatures = self.signatures.get(&block_height).unwrap_or(&empty_hashmap);
            let mut block_signatures = BlockSignaturesV2::new(
                *block.hash(),
                block.height(),
                block.era_id(),
                self.chainspec.name_hash(),
            );
            for signature in signatures.values() {
                block_signatures
                    .insert_signature(signature.public_key().clone(), *signature.signature());
            }
            BlockWithMetadata {
                block: block.clone(),
                block_signatures: BlockSignatures::V2(block_signatures),
            }
        })
    }

    fn proposed_block(&self, timestamp: Timestamp) -> ProposedBlock<ClContext> {
        let rewards_window = self.chainspec.core_config.signature_rewards_max_delay;
        let rewarded_signatures = self
            .proposed_block_height
            .map(|proposed_block_height| {
                RewardedSignatures::new(
                    (1..=rewards_window)
                        .into_iter()
                        .filter_map(|height_diff| proposed_block_height.checked_sub(height_diff))
                        .map(|height| {
                            let signing_validators = self
                                .signatures_to_include
                                .get(&height)
                                .cloned()
                                .unwrap_or_default();
                            SingleBlockRewardedSignatures::from_validator_set(
                                &signing_validators,
                                self.secret_keys.keys(),
                            )
                        }),
                )
            })
            .unwrap_or_default();
        new_proposed_block_with_cited_signatures(
            timestamp,
            self.transfers_to_include.to_vec(),
            vec![],
            vec![],
            self.deploys_to_include.to_vec(),
            rewarded_signatures,
        )
    }

    /// Validates a block using a `BlockValidator` component, and returns the result.
    async fn validate_block(&mut self, rng: &mut TestRng, timestamp: Timestamp) -> bool {
        let proposed_block = self.proposed_block(timestamp);

        // Create the reactor and component.
        let our_secret_key = self
            .secret_keys
            .values()
            .next()
            .expect("should have a secret key")
            .clone();
        let reactor = MockReactor::new(our_secret_key, self.secret_keys.keys().cloned());
        let effect_builder =
            EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));
        let mut block_validator = BlockValidator::new(
            self.chainspec.clone(),
            reactor.validator_matrix.clone(),
            Config::default(),
        );

        // Pass the block to the component. This future will eventually resolve to the result, i.e.
        // whether the block is valid or not.
        let bob_node_id = NodeId::random(rng);
        let block_height = rng.gen_range(0..1000);
        let validation_result = tokio::spawn(effect_builder.validate_block(
            bob_node_id,
            self.proposed_block_height.unwrap_or(block_height),
            proposed_block.clone(),
        ));
        let event = reactor.expect_block_validator_event().await;
        let effects = block_validator.handle_event(effect_builder, rng, event);

        // If validity could already be determined, the effect will be the validation response.
        if !block_validator.validation_states.is_empty()
            && block_validator
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

        // Otherwise the effects are either requests to fetch the block's deploys, or to fetch past
        // blocks for the purpose of signature validation.
        let event_futures: Vec<_> = effects.into_iter().map(tokio::spawn).collect();

        // We make our mock reactor answer with the expected blocks and/or deploys and transfers:
        reactor.handle_requests(self).await;

        // At this point we either responded with requested deploys, or the past blocks. This
        // should generate other events (`GotPastBlocksWithMetadata` in the case of past blocks, or
        // a bunch of `TransactionFetched` in the case of deploys). We have to handle them.
        let mut effects = Effects::new();
        for future in event_futures {
            let events = future.await.unwrap();
            effects.extend(
                events
                    .into_iter()
                    .flat_map(|event| block_validator.handle_event(effect_builder, rng, event)),
            );
        }

        // If there are no effects - some blocks have been missing from storage. Announce the
        // finalization of the blocks we have in the context.
        if effects.is_empty() {
            for block_height in self.get_delayed_blocks() {
                effects.extend(block_validator.handle_event(
                    effect_builder,
                    rng,
                    Event::BlockStored(block_height),
                ));
            }
        }

        // If there are still no effects, something went wrong.
        assert!(!effects.is_empty());

        // If there were no signatures in the block, the validity of the block should be determined
        // at this point. In such a case, return the result.
        if !block_validator.validation_states.is_empty()
            && block_validator
                .validation_states
                .values()
                .all(BlockValidationState::completed)
        {
            assert_eq!(1, effects.len());
            for effect in effects {
                tokio::spawn(effect).await.unwrap();
            }
            return validation_result.await.unwrap();
        }

        // Otherwise, we have more effects to handle. After the blocks have been returned, the
        // validator should now ask for the deploys and signatures.
        // If some blocks have been delayed, this can be another request for past blocks.
        // Let's handle those requests.
        let event_futures: Vec<_> = effects.into_iter().map(tokio::spawn).collect();

        // We make our mock reactor answer with the expected items.
        reactor.handle_requests(self).await;

        // Again, we'll have a bunch of events to handle, so we handle them.
        let mut effects = Effects::new();
        for future in event_futures {
            let events = future.await.unwrap();
            effects.extend(
                events
                    .into_iter()
                    .flat_map(|event| block_validator.handle_event(effect_builder, rng, event)),
            );
        }

        // If there are no effects at this point, something went wrong.
        assert!(!effects.is_empty());

        // If no blocks were delayed, we just returned all the fetched items, so now the validity
        // should have been resolved. Return the result if it is so.
        if !block_validator.validation_states.is_empty()
            && block_validator
                .validation_states
                .values()
                .all(BlockValidationState::completed)
        {
            assert_eq!(1, effects.len());
            for effect in effects {
                tokio::spawn(effect).await.unwrap();
            }
            return validation_result.await.unwrap();
        }

        // Otherwise, we have more effects to handle. At this point, all the delayed blocks should
        // have been stored and returned, so we just have a bunch of fetch requests to handle.
        let event_futures: Vec<_> = effects.into_iter().map(tokio::spawn).collect();

        // We make our mock reactor answer with the expected items.
        reactor.handle_requests(self).await;

        // Again, we'll have a bunch of events to handle. At this point we should have a bunch of
        // `TransactionFetched` or `FinalitySignatureFetched` events. We handle them.
        let mut effects = Effects::new();
        for future in event_futures {
            let events = future.await.unwrap();
            effects.extend(
                events
                    .into_iter()
                    .flat_map(|event| block_validator.handle_event(effect_builder, rng, event)),
            );
        }

        // Nothing more should be requested, so we expect at most one effect: the validation
        // response. Zero effects is possible if block validator responded with false before, but
        // hasn't marked the state invalid (it can happen when peers are exhausted). In any case,
        // the result should be resolved now.
        assert!(effects.len() < 2);
        for effect in effects {
            tokio::spawn(effect).await.unwrap(); // Response.
        }
        validation_result.await.unwrap()
    }
}

/// Verifies that a block without any deploys or transfers is valid.
#[tokio::test]
async fn empty_block() {
    let mut rng = TestRng::new();
    let mut empty_context = ValidationContext::new().with_num_validators(&mut rng, 1);
    assert!(empty_context.validate_block(&mut rng, 1000.into()).await);
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

    let mut deploys_context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys.clone())
        .include_all_deploys();
    let mut transfers_context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transfers(transfers.clone())
        .include_all_transfers();
    let mut both_context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys)
        .with_transfers(transfers)
        .include_all_deploys()
        .include_all_transfers();

    // Both 1000 and 1100 are timestamps compatible with the deploys and transfers.
    assert!(both_context.validate_block(&mut rng, 1000.into()).await);
    assert!(both_context.validate_block(&mut rng, 1100.into()).await);

    // A block with timestamp 999 can't contain a transfer or deploy with timestamp 1000.
    assert!(!deploys_context.validate_block(&mut rng, 999.into()).await);
    assert!(!transfers_context.validate_block(&mut rng, 999.into()).await);
    assert!(!both_context.validate_block(&mut rng, 999.into()).await);

    // At time 1101, the deploy and transfer from time 900 have expired.
    assert!(!deploys_context.validate_block(&mut rng, 1101.into()).await);
    assert!(
        !transfers_context
            .validate_block(&mut rng, 1101.into())
            .await
    );
    assert!(!both_context.validate_block(&mut rng, 1101.into()).await);
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
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys)
        .with_transfers(transfers)
        .include_all_deploys()
        .include_all_transfers();
    assert!(context.validate_block(&mut rng, timestamp).await);

    // Now we hide a transfer in the deploys section. This should be invalid.
    let deploys = vec![deploy1.clone(), deploy2.clone(), transfer2.clone()];
    let transfers = vec![transfer1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys)
        .with_transfers(transfers)
        .include_all_deploys()
        .include_all_transfers();
    assert!(!context.validate_block(&mut rng, timestamp).await);

    // A regular deploy in the transfers section is also invalid.
    let deploys = vec![deploy2.clone()];
    let transfers = vec![transfer1.clone(), deploy1.clone(), transfer2.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys)
        .with_transfers(transfers)
        .include_all_deploys()
        .include_all_transfers();
    assert!(!context.validate_block(&mut rng, timestamp).await);

    // Each deploy must be unique
    let deploys = vec![deploy1.clone(), deploy2.clone()];
    let transfers = vec![transfer1.clone(), transfer2.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys)
        .with_transfers(transfers)
        .include_all_deploys()
        .include_all_transfers()
        .include_deploys(vec![TransactionHashWithApprovals::from(
            &Transaction::Deploy(deploy1.clone()),
        )]);
    assert!(!context.validate_block(&mut rng, timestamp).await);

    // And each transfer must be unique, too.
    let deploys = vec![deploy1.clone(), deploy2.clone()];
    let transfers = vec![transfer1.clone(), transfer2.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_deploys(deploys)
        .with_transfers(transfers)
        .include_all_deploys()
        .include_all_transfers()
        .include_transfers(vec![TransactionHashWithApprovals::from(
            &Transaction::Deploy(transfer2.clone()),
        )]);
    assert!(!context.validate_block(&mut rng, timestamp).await);
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
        let secret_key = Arc::new(SecretKey::random(&mut rng));
        let public_key = PublicKey::from(&*secret_key);
        let reactor = MockReactor::new(secret_key, vec![public_key]);
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
        let context = ValidationContext::new()
            .with_num_validators(&mut rng, 1)
            .with_deploys(vec![deploys[0].clone()])
            .with_transfers(vec![transfers[0].clone()]);
        reactor.handle_requests(&context).await;

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
        let context = context
            .with_deploys(vec![deploys[1].clone()])
            .with_transfers(vec![transfers[1].clone()]);
        reactor.handle_requests(&context).await;

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
        let context = context
            .with_deploys(vec![deploys[2].clone()])
            .with_transfers(vec![transfers[2].clone()]);
        reactor.handle_requests(&context).await;

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

#[tokio::test]
async fn should_validate_block_with_signatures() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy1 = new_deploy(&mut rng, timestamp, ttl);
    let deploy2 = new_deploy(&mut rng, timestamp, ttl);
    let transfer1 = new_transfer(&mut rng, timestamp, ttl);
    let transfer2 = new_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into())
        .with_deploys(vec![deploy1, deploy2])
        .with_transfers(vec![transfer1, transfer2])
        .include_all_deploys()
        .include_all_transfers();

    let validators = context.get_validators();

    let mut context = context
        .with_signatures_for_block(3, 5, &validators)
        .include_signatures(3, 5, &validators);

    assert!(context.validate_block(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_fetch_missing_signature() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy1 = new_deploy(&mut rng, timestamp, ttl);
    let deploy2 = new_deploy(&mut rng, timestamp, ttl);
    let transfer1 = new_transfer(&mut rng, timestamp, ttl);
    let transfer2 = new_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into())
        .with_deploys(vec![deploy1, deploy2])
        .with_transfers(vec![transfer1, transfer2])
        .include_all_deploys()
        .include_all_transfers();

    let validators = context.get_validators();
    let mut signing_validators = context.get_validators();
    let leftover = signing_validators.pop().unwrap(); // one validator will be missing from the set that signed

    let mut context = context
        .with_signatures_for_block(3, 5, &signing_validators)
        .with_fetchable_signatures(3, 5, &[leftover])
        .include_signatures(3, 5, &validators);

    assert!(context.validate_block(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_fail_if_unable_to_fetch_signature() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy1 = new_deploy(&mut rng, timestamp, ttl);
    let deploy2 = new_deploy(&mut rng, timestamp, ttl);
    let transfer1 = new_transfer(&mut rng, timestamp, ttl);
    let transfer2 = new_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into())
        .with_deploys(vec![deploy1, deploy2])
        .with_transfers(vec![transfer1, transfer2])
        .include_all_deploys()
        .include_all_transfers();

    let validators = context.get_validators();
    let mut signing_validators = context.get_validators();
    let _ = signing_validators.pop(); // one validator will be missing from the set that signed

    let mut context = context
        .with_signatures_for_block(3, 5, &signing_validators)
        .include_signatures(3, 5, &validators);

    assert!(!context.validate_block(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_validate_with_delayed_block() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy1 = new_deploy(&mut rng, timestamp, ttl);
    let deploy2 = new_deploy(&mut rng, timestamp, ttl);
    let transfer1 = new_transfer(&mut rng, timestamp, ttl);
    let transfer2 = new_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 4, 0.into())
        .with_delayed_blocks(&mut rng, 5, 5, 0.into())
        .with_deploys(vec![deploy1, deploy2])
        .with_transfers(vec![transfer1, transfer2])
        .include_all_deploys()
        .include_all_transfers();

    let validators = context.get_validators();

    let mut context = context
        .with_signatures_for_block(3, 5, &validators)
        .include_signatures(3, 5, &validators);

    assert!(context.validate_block(&mut rng, timestamp).await);
}
