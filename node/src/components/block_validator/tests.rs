use std::{collections::VecDeque, sync::Arc, time::Duration};

use derive_more::From;
use itertools::Itertools;
use rand::Rng;

use casper_types::{
    bytesrepr::Bytes, runtime_args, system::standard_payment::ARG_AMOUNT, testing::TestRng, Block,
    BlockSignatures, BlockSignaturesV2, Chainspec, ChainspecRawBytes, Deploy, ExecutableDeployItem,
    FinalitySignatureV2, RuntimeArgs, SecretKey, TestBlockBuilder, TimeDiff, Transaction,
    TransactionHash, TransactionId, TransactionV1, TransactionV1Config, AUCTION_LANE_ID,
    INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID, U512,
};

use crate::{
    components::{
        consensus::BlockContext,
        fetcher::{self, FetchItem},
    },
    effect::requests::StorageRequest,
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    testing::LARGE_WASM_LANE_ID,
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
    FatalAnnouncement(#[allow(dead_code)] FatalAnnouncement),
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
                    if let Some(transaction) = context.get_transaction(id) {
                        let response = FetchedData::FromPeer {
                            item: Box::new(transaction),
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
    transfer: Vec<(TransactionHash, BTreeSet<Approval>)>,
    staking: Vec<(TransactionHash, BTreeSet<Approval>)>,
    install_upgrade: Vec<(TransactionHash, BTreeSet<Approval>)>,
    standard: Vec<(TransactionHash, BTreeSet<Approval>)>,
    cited_signatures: RewardedSignatures,
) -> ProposedBlock<ClContext> {
    // Accusations and ancestors are empty, and the random bit is always true:
    // These values are not checked by the block validator.
    let block_context = BlockContext::new(timestamp, vec![]);
    let transactions = {
        let mut ret = BTreeMap::new();
        ret.insert(MINT_LANE_ID, transfer.into_iter().collect());
        ret.insert(AUCTION_LANE_ID, staking.into_iter().collect());
        ret.insert(
            INSTALL_UPGRADE_LANE_ID,
            install_upgrade.into_iter().collect(),
        );
        ret.insert(LARGE_WASM_LANE_ID, standard.into_iter().collect());
        ret
    };
    let block_payload = BlockPayload::new(transactions, vec![], cited_signatures, true, 1u8);
    ProposedBlock::new(Arc::new(block_payload), block_context)
}

pub(super) fn new_proposed_block(
    timestamp: Timestamp,
    transfer: Vec<(TransactionHash, BTreeSet<Approval>)>,
    staking: Vec<(TransactionHash, BTreeSet<Approval>)>,
    install_upgrade: Vec<(TransactionHash, BTreeSet<Approval>)>,
    standard: Vec<(TransactionHash, BTreeSet<Approval>)>,
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

pub(super) fn new_v1_standard(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
) -> Transaction {
    let transaction_v1 = TransactionV1::random_wasm(rng, Some(timestamp), Some(ttl));
    Transaction::V1(transaction_v1)
}

pub(super) fn new_auction(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Transaction {
    let transaction_v1 = TransactionV1::random_auction(rng, Some(timestamp), Some(ttl));
    Transaction::V1(transaction_v1)
}

pub(super) fn new_install_upgrade(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
) -> Transaction {
    TransactionV1::random_install_upgrade(rng, Some(timestamp), Some(ttl)).into()
}

pub(super) fn new_deploy(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Transaction {
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

    Deploy::new_signed(
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
    .into()
}

pub(super) fn new_v1_transfer(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
) -> Transaction {
    TransactionV1::random_transfer(rng, Some(timestamp), Some(ttl)).into()
}

pub(super) fn new_transfer(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Transaction {
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

    Deploy::new_signed(
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
    .into()
}

pub(super) fn new_mint(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Transaction {
    if rng.gen() {
        new_v1_transfer(rng, timestamp, ttl)
    } else {
        new_transfer(rng, timestamp, ttl)
    }
}

pub(super) fn new_standard(rng: &mut TestRng, timestamp: Timestamp, ttl: TimeDiff) -> Transaction {
    if rng.gen() {
        new_v1_standard(rng, timestamp, ttl)
    } else {
        new_deploy(rng, timestamp, ttl)
    }
}

pub(super) fn new_non_transfer(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
) -> Transaction {
    match rng.gen_range(0..3) {
        0 => new_standard(rng, timestamp, ttl),
        1 => new_install_upgrade(rng, timestamp, ttl),
        2 => new_auction(rng, timestamp, ttl),
        _ => unreachable!(),
    }
}

type SecretKeys = BTreeMap<PublicKey, Arc<SecretKey>>;

struct ValidationContext {
    chainspec: Chainspec,
    // Validators
    secret_keys: SecretKeys,
    // map of height → block
    past_blocks: HashMap<u64, Block>,
    // blocks that will be "stored" during validation
    delayed_blocks: HashMap<u64, Block>,
    transactions: HashMap<TransactionId, Transaction>,
    transfers: HashMap<TransactionId, Transaction>,
    // map of block height → signatures for the block
    signatures: HashMap<u64, HashMap<PublicKey, FinalitySignatureV2>>,
    // map of signatures that aren't stored, but are fetchable
    fetchable_signatures: HashMap<FinalitySignatureId, FinalitySignature>,

    // fields defining the proposed block that will be validated
    transactions_to_include: Vec<(TransactionHash, BTreeSet<Approval>)>,
    transfers_to_include: Vec<(TransactionHash, BTreeSet<Approval>)>,
    signatures_to_include: HashMap<u64, BTreeSet<PublicKey>>,
    proposed_block_height: Option<u64>,
}

impl ValidationContext {
    fn new() -> Self {
        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        Self {
            chainspec,
            secret_keys: BTreeMap::new(),
            past_blocks: HashMap::new(),
            delayed_blocks: HashMap::new(),
            transactions: HashMap::new(),
            transfers: HashMap::new(),
            fetchable_signatures: HashMap::new(),
            signatures: HashMap::new(),
            transactions_to_include: vec![],
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

    fn with_count_limits(
        mut self,
        mint_count: Option<u64>,
        auction: Option<u64>,
        install: Option<u64>,
        large_limit: Option<u64>,
    ) -> Self {
        let transaction_v1_config = TransactionV1Config::default().with_count_limits(
            mint_count,
            auction,
            install,
            large_limit,
        );
        self.chainspec.transaction_config.transaction_v1_config = transaction_v1_config;
        self
    }

    fn with_block_gas_limit(mut self, block_gas_limit: u64) -> Self {
        self.chainspec.transaction_config.block_gas_limit = block_gas_limit;
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
            .extend((min_height..=max_height).map(|height| {
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
            .extend((min_height..=max_height).map(|height| {
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

    fn with_transactions(mut self, transactions: Vec<Transaction>) -> Self {
        self.transactions.extend(
            transactions
                .into_iter()
                .map(|transaction| (transaction.clone().fetch_id(), transaction)),
        );
        self
    }

    fn with_transfers(mut self, transfers: Vec<Transaction>) -> Self {
        self.transfers.extend(
            transfers
                .into_iter()
                .map(|transaction| (transaction.clone().fetch_id(), transaction)),
        );
        self
    }

    fn include_all_transactions(mut self) -> Self {
        self.transactions_to_include.extend(
            self.transactions
                .values()
                .map(|transaction| (transaction.hash(), transaction.approvals())),
        );
        self
    }

    fn include_all_transfers(mut self) -> Self {
        self.transfers_to_include.extend(
            self.transfers
                .values()
                .map(|transaction| (transaction.hash(), transaction.approvals())),
        );
        self
    }

    fn include_transactions<I: IntoIterator<Item = (TransactionHash, BTreeSet<Approval>)>>(
        mut self,
        transactions: I,
    ) -> Self {
        self.transactions_to_include.extend(transactions);
        self
    }

    fn include_transfers<I: IntoIterator<Item = (TransactionHash, BTreeSet<Approval>)>>(
        mut self,
        transfers: I,
    ) -> Self {
        self.transfers_to_include.extend(transfers);
        self
    }

    fn get_transaction(&self, id: TransactionId) -> Option<Transaction> {
        self.transactions
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
            self.transactions_to_include.to_vec(),
            rewarded_signatures,
        )
    }

    async fn proposal_is_valid(&mut self, rng: &mut TestRng, timestamp: Timestamp) -> bool {
        self.validate_proposed_block(rng, timestamp).await.is_ok()
    }

    /// Validates a block using a `BlockValidator` component, and returns the result.
    async fn validate_proposed_block(
        &mut self,
        rng: &mut TestRng,
        timestamp: Timestamp,
    ) -> Result<(), Box<InvalidProposalError>> {
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
            Arc::new(self.chainspec.clone()),
            reactor.validator_matrix.clone(),
            Config::default(),
            1u8,
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

        // Otherwise the effects are either requests to fetch the block's transactions, or to fetch
        // past blocks for the purpose of signature validation.
        let event_futures: Vec<_> = effects.into_iter().map(tokio::spawn).collect();

        // We make our mock reactor answer with the expected blocks and/or transactions and
        // transfers:
        reactor.handle_requests(self).await;

        // At this point we either responded with requested transactions, or the past blocks. This
        // should generate other events (`GotPastBlocksWithMetadata` in the case of past blocks, or
        // a bunch of `TransactionFetched` in the case of transactions). We have to handle them.
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
        // validator should now ask for the transactions and signatures.
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

/// Verifies that a block without any transactions or transfers is valid.
#[tokio::test]
async fn empty_block() {
    let mut rng = TestRng::new();
    let mut empty_context = ValidationContext::new().with_num_validators(&mut rng, 1);
    assert!(empty_context.proposal_is_valid(&mut rng, 1000.into()).await);
}

/// Verifies that the block validator checks transaction and transfer timestamps and ttl.
#[tokio::test]
async fn ttl() {
    // The ttl is 200 ms, and our transactions and transfers have timestamps 900 and 1000. So the
    // block timestamp must be at least 1000 and at most 1100.
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let transactions = vec![
        new_non_transfer(&mut rng, 1000.into(), ttl),
        new_non_transfer(&mut rng, 900.into(), ttl),
    ];
    let transfers: Vec<Transaction> = vec![
        new_v1_transfer(&mut rng, 1000.into(), ttl),
        new_v1_transfer(&mut rng, 900.into(), ttl),
    ];

    let mut transactions_context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions.clone())
        .with_count_limits(Some(3000), Some(3000), Some(3000), Some(3000))
        .with_block_gas_limit(15_300_000_000_000)
        .include_all_transactions();
    let mut transfers_context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transfers(transfers.clone())
        .with_count_limits(Some(3000), Some(3000), Some(3000), Some(3000))
        .with_block_gas_limit(15_300_000_000_000)
        .include_all_transfers();
    let mut both_context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .with_count_limits(Some(3000), Some(3000), Some(3000), Some(3000))
        .with_block_gas_limit(15_300_000_000_000)
        .include_all_transactions()
        .include_all_transfers();

    // Both 1000 and 1100 are timestamps compatible with the transactions and transfers.
    assert!(both_context.proposal_is_valid(&mut rng, 1000.into()).await);
    assert!(both_context.proposal_is_valid(&mut rng, 1100.into()).await);

    // A block with timestamp 999 can't contain a transfer or transactions with timestamp 1000.
    assert!(
        !transactions_context
            .proposal_is_valid(&mut rng, 999.into())
            .await
    );
    assert!(
        !transfers_context
            .proposal_is_valid(&mut rng, 999.into())
            .await
    );
    assert!(!both_context.proposal_is_valid(&mut rng, 999.into()).await);

    // At time 1101, the transactions and transfer from time 900 have expired.
    assert!(
        !transactions_context
            .proposal_is_valid(&mut rng, 1101.into())
            .await
    );
    assert!(
        !transfers_context
            .proposal_is_valid(&mut rng, 1101.into())
            .await
    );
    assert!(!both_context.proposal_is_valid(&mut rng, 1101.into()).await);
}

/// Verifies that a block is invalid if it contains a transfer in the transactions section
/// or vice versa.
#[tokio::test]
async fn transfer_transaction_mixup_and_replay() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy = new_deploy(&mut rng, timestamp, ttl);
    let transaction_v1 = new_v1_standard(&mut rng, timestamp, ttl);
    let transfer_orig = new_transfer(&mut rng, timestamp, ttl);
    let transfer_v1 = new_v1_transfer(&mut rng, timestamp, ttl);

    // First we make sure that our transfers and transactions would normally be valid.
    let transactions = vec![transaction_v1.clone()];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers();
    assert!(context.proposal_is_valid(&mut rng, timestamp).await);

    // Now we test for different invalid combinations of transactions and transfers:
    // 1. Original style transfer in the transactions section.
    let transactions = vec![
        transfer_orig.clone(),
        transaction_v1.clone(),
        deploy.clone(),
    ];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers();
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
    // 2. V1 transfer in the transactions section.
    let transactions = vec![transfer_v1.clone(), transaction_v1.clone(), deploy.clone()];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers();
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
    // 3. Legacy deploy in the transfers section.
    let transactions = vec![transaction_v1.clone(), deploy.clone()];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone(), deploy.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers();
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
    // 4. V1 transaction in the transfers section.
    let transactions = vec![transaction_v1.clone(), deploy.clone()];
    let transfers = vec![
        transfer_orig.clone(),
        transfer_v1.clone(),
        transaction_v1.clone(),
    ];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers();
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);

    // Each transaction must be unique
    let transactions = vec![deploy.clone(), transaction_v1.clone()];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers()
        .include_transactions(vec![(deploy.hash(), deploy.approvals())]);
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
    let transactions = vec![deploy.clone(), transaction_v1.clone()];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers()
        .include_transactions(vec![(transaction_v1.hash(), transaction_v1.approvals())]);
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);

    // And each transfer must be unique, too.
    let transactions = vec![deploy.clone(), transaction_v1.clone()];
    let transfers = vec![transfer_v1.clone(), transfer_orig.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers()
        .include_transfers(vec![(transfer_v1.hash(), transfer_v1.approvals())]);
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
    let transactions = vec![deploy.clone(), transaction_v1.clone()];
    let transfers = vec![transfer_orig.clone(), transfer_v1.clone()];
    let mut context = ValidationContext::new()
        .with_num_validators(&mut rng, 1)
        .with_transactions(transactions)
        .with_transfers(transfers)
        .include_all_transactions()
        .include_all_transfers()
        .include_transactions(vec![(transfer_orig.hash(), transfer_orig.approvals())]);
    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
}

/// Verifies that the block validator fetches from multiple peers.
#[tokio::test]
async fn should_fetch_from_multiple_peers() {
    let _ = crate::logging::init();
    tokio::time::timeout(Duration::from_secs(5), async move {
        let peer_count = 3;
        let mut rng = TestRng::new();
        let ttl = TimeDiff::from_seconds(200);
        let transactions = (0..peer_count)
            .map(|i| new_non_transfer(&mut rng, (900 + i).into(), ttl))
            .collect_vec();
        let transfers = (0..peer_count)
            .map(|i| new_v1_transfer(&mut rng, (1000 + i).into(), ttl))
            .collect_vec();

        // Assemble the block to be validated.
        let transfers_for_block = transfers
            .iter()
            .map(|transfer| (transfer.hash(), transfer.approvals()))
            .collect_vec();
        let standard_for_block = transactions
            .iter()
            .map(|transaction| (transaction.hash(), transaction.approvals()))
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
        let (mut chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        chainspec.transaction_config.block_gas_limit = 100_000_000_000_000;
        let transaction_v1_config = TransactionV1Config::default().with_count_limits(
            Some(3000),
            Some(3000),
            Some(3000),
            Some(3000),
        );
        chainspec.transaction_config.transaction_v1_config = transaction_v1_config;

        let mut block_validator = BlockValidator::new(
            Arc::new(chainspec),
            reactor.validator_matrix.clone(),
            Config::default(),
            1u8,
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

        // The effects are requests to fetch the block's transactions.  There are six fetch
        // requests, all using the first peer.
        let fetch_results = fetch_effects.drain(..).map(tokio::spawn).collect_vec();

        // Provide the first deploy and transfer on first asking.
        let context = ValidationContext::new()
            .with_num_validators(&mut rng, 1)
            .with_transactions(vec![transactions[0].clone()])
            .with_transfers(vec![transfers[0].clone()]);
        reactor.handle_requests(&context).await;

        let mut missing = vec![];
        for fetch_result in fetch_results {
            let mut events = fetch_result.await.unwrap();
            assert_eq!(1, events.len());
            // The event should be `TransactionFetched`.
            let event = events.pop().unwrap();
            // New fetch requests will be made using a different peer for all transactions not
            // already registered as fetched.
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
            .with_transactions(vec![transactions[1].clone()])
            .with_transfers(vec![transfers[1].clone()]);
        reactor.handle_requests(&context).await;

        missing.clear();
        for fetch_result in fetch_results {
            let mut events = fetch_result.await.unwrap();
            assert_eq!(1, events.len());
            // The event should be `TransactionFetched`.
            let event = events.pop().unwrap();
            // New fetch requests will be made using a different peer for all transactions not
            // already registered as fetched.
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
            .with_transactions(vec![transactions[2].clone()])
            .with_transfers(vec![transfers[2].clone()]);
        reactor.handle_requests(&context).await;

        let mut effects = Effects::new();
        for fetch_result in fetch_results {
            let mut events = fetch_result.await.unwrap();
            assert_eq!(1, events.len());
            // The event should be `TransactionFetched`.
            let event = events.pop().unwrap();
            // Once the block is deemed valid (i.e. when the final missing transaction is
            // successfully fetched) the effects will be three validation responses.
            effects.extend(block_validator.handle_event(effect_builder, &mut rng, event));
            assert!(effects.is_empty() || effects.len() == peer_count as usize);
        }

        for effect in effects {
            tokio::spawn(effect).await.unwrap();
        }

        for validation_result in validation_results {
            assert!(validation_result.await.unwrap().is_ok());
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
    let transaction_v1 = new_v1_standard(&mut rng, timestamp, ttl);
    let transfer = new_transfer(&mut rng, timestamp, ttl);
    let transfer_v1 = new_v1_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into())
        .with_transactions(vec![transaction_v1])
        .with_transfers(vec![transfer, transfer_v1])
        .include_all_transactions()
        .include_all_transfers();

    let validators = context.get_validators();

    let mut context = context
        .with_signatures_for_block(3, 5, &validators)
        .include_signatures(3, 5, &validators);

    assert!(context.proposal_is_valid(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_fetch_missing_signature() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let transaction_v1 = new_v1_standard(&mut rng, timestamp, ttl);
    let transfer = new_transfer(&mut rng, timestamp, ttl);
    let transfer_v1 = new_v1_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into())
        .with_transactions(vec![transaction_v1])
        .with_transfers(vec![transfer, transfer_v1])
        .include_all_transactions()
        .include_all_transfers();

    let validators = context.get_validators();
    let mut signing_validators = context.get_validators();
    let leftover = signing_validators.pop().unwrap(); // one validator will be missing from the set that signed

    let mut context = context
        .with_signatures_for_block(3, 5, &signing_validators)
        .with_fetchable_signatures(3, 5, &[leftover])
        .include_signatures(3, 5, &validators);

    assert!(context.proposal_is_valid(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_fail_if_unable_to_fetch_signature() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let deploy = new_deploy(&mut rng, timestamp, ttl);
    let transaction_v1 = new_v1_standard(&mut rng, timestamp, ttl);
    let transfer = new_transfer(&mut rng, timestamp, ttl);
    let transfer_v1 = new_v1_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into())
        .with_transactions(vec![deploy, transaction_v1])
        .with_transfers(vec![transfer, transfer_v1])
        .include_all_transactions()
        .include_all_transfers();

    let validators = context.get_validators();
    let mut signing_validators = context.get_validators();
    let _ = signing_validators.pop().expect("must pop"); // one validator will be missing from the set that signed

    let mut context = context
        .with_signatures_for_block(3, 5, &signing_validators)
        .include_signatures(3, 5, &validators);

    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_fail_if_unable_to_fetch_signature_for_block_without_transactions() {
    let mut rng = TestRng::new();
    let timestamp = Timestamp::from(1000);

    // No transactions in the block.
    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 5, 0.into());

    let validators = context.get_validators();
    let mut signing_validators = context.get_validators();
    let _ = signing_validators.pop(); // one validator will be missing from the set that signed

    let mut context = context
        .with_signatures_for_block(3, 5, &signing_validators)
        .include_signatures(3, 5, &validators);

    assert!(!context.proposal_is_valid(&mut rng, timestamp).await);
}

#[tokio::test]
async fn should_validate_with_delayed_block() {
    let mut rng = TestRng::new();
    let ttl = TimeDiff::from_millis(200);
    let timestamp = Timestamp::from(1000);
    let transaction_v1 = new_v1_standard(&mut rng, timestamp, ttl);
    let transfer = new_transfer(&mut rng, timestamp, ttl);
    let transfer_v1 = new_v1_transfer(&mut rng, timestamp, ttl);

    let context = ValidationContext::new()
        .with_num_validators(&mut rng, 3)
        .with_past_blocks(&mut rng, 0, 4, 0.into())
        .with_delayed_blocks(&mut rng, 5, 5, 0.into())
        .with_transactions(vec![transaction_v1])
        .with_transfers(vec![transfer, transfer_v1])
        .include_all_transactions()
        .include_all_transfers();

    let validators = context.get_validators();

    let mut context = context
        .with_signatures_for_block(3, 5, &validators)
        .include_signatures(3, 5, &validators);

    assert!(context.proposal_is_valid(&mut rng, timestamp).await);
}
