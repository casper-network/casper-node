mod error;
mod event;
mod metrics;
mod pending_signatures;
mod signature;
mod signature_cache;
mod state;
mod utils;

use std::convert::Infallible;

use async_trait::async_trait;
use datasize::DataSize;
use itertools::Itertools;
use num::rational::Ratio;
use prometheus::Registry;
use tracing::{error, info};

use casper_execution_engine::core::engine_state::GetEraValidatorsError;
use casper_types::{
    system::auction::{EraValidators, ValidatorWeights},
    EraId, ProtocolVersion,
};

use self::{
    metrics::Metrics,
    state::{LinearChain, Outcome, Outcomes},
};
use crate::{
    components::{contract_runtime::EraValidatorsRequest, Component},
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, EffectResultExt, Effects, TargetPeers,
    },
    protocol::Message,
    types::{ActivationPoint, BlockHeader},
    NodeRng,
};
pub(crate) use error::{BlockSignatureError, Error};
pub(crate) use event::Event;
pub(crate) use utils::{
    check_sufficient_block_signatures, check_sufficient_block_signatures_with_quorum_formula,
    validate_block_signatures,
};

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainComponent {
    linear_chain_state: LinearChain,
    #[data_size(skip)]
    metrics: Metrics,
    /// If true, the process should stop execution to allow an upgrade to proceed.
    stop_for_upgrade: bool,
}

impl LinearChainComponent {
    pub(crate) fn new(
        registry: &Registry,
        protocol_version: ProtocolVersion,
        auction_delay: u64,
        unbonding_delay: u64,
        finality_threshold_fraction: Ratio<u64>,
        next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> Result<Self, prometheus::Error> {
        let metrics = Metrics::new(registry)?;
        let linear_chain_state = LinearChain::new(
            protocol_version,
            auction_delay,
            unbonding_delay,
            finality_threshold_fraction,
            next_upgrade_activation_point,
        );
        Ok(LinearChainComponent {
            linear_chain_state,
            metrics,
            stop_for_upgrade: false,
        })
    }

    pub(crate) fn stop_for_upgrade(&self) -> bool {
        self.stop_for_upgrade
    }
}

fn outcomes_to_effects<REv>(
    effect_builder: EffectBuilder<REv>,
    outcomes: Outcomes,
) -> Effects<Event>
where
    REv: From<StorageRequest>
        + From<NetworkRequest<Message>>
        + From<LinearChainAnnouncement>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + Send,
{
    outcomes
        .into_iter()
        .map(|outcome| match outcome {
            Outcome::StoreBlockSignatures(block_signatures, should_upgrade) => effect_builder
                .put_signatures_to_storage(block_signatures)
                .events(move |_| should_upgrade.then(|| Event::Upgrade).into_iter()),
            Outcome::StoreBlock(block, execution_results) => async move {
                let block_hash = *block.hash();
                effect_builder.put_block_to_storage(block.clone()).await;
                effect_builder
                    .put_execution_results_to_storage(block_hash, execution_results)
                    .await;
                block
            }
            .event(|block| Event::PutBlockResult { block }),
            Outcome::Gossip(fs) => {
                let era_id = fs.era_id;
                let message = Message::FinalitySignature(fs);
                effect_builder
                    .broadcast_message_to_validators(message, era_id)
                    .ignore()
            }
            Outcome::AnnounceSignature(fs) => {
                effect_builder.announce_finality_signature(fs).ignore()
            }
            Outcome::AnnounceBlock(block) => effect_builder.announce_block_added(block).ignore(),
            Outcome::LoadSignatures(fs) => effect_builder
                .get_signatures_from_storage(fs.block_hash)
                .event(move |maybe_signatures| {
                    Event::GetStoredFinalitySignaturesResult(fs, maybe_signatures.map(Box::new))
                }),
            Outcome::VerifyIfBonded {
                new_fs,
                known_fs,
                protocol_version,
                latest_state_root_hash,
            } => effect_builder
                .is_bonded_validator(
                    new_fs.public_key.clone(),
                    new_fs.era_id,
                    latest_state_root_hash,
                    protocol_version,
                )
                .result(
                    |is_bonded| Event::IsBonded(known_fs, new_fs, is_bonded),
                    |error| {
                        error!(%error, "checking in future eras returned an error.");
                        panic!("couldn't check if validator is bonded")
                    },
                ),
        })
        .concat()
}

impl<REv> Component<REv> for LinearChainComponent
where
    REv: From<StorageRequest>
        + From<NetworkRequest<Message>>
        + From<LinearChainAnnouncement>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + Send,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::NewLinearChainBlock {
                // executed
                block,
                execution_results,
            } => {
                let outcomes = self
                    .linear_chain_state
                    .handle_new_block(block, execution_results);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::PutBlockResult { block } => {
                let completion_duration = block.header().timestamp().elapsed().millis();
                self.metrics
                    .block_completion_duration
                    .set(completion_duration as i64);
                let outcomes = self.linear_chain_state.handle_put_block(block);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::FinalitySignatureReceived(fs, gossiped) => {
                let outcomes = self
                    .linear_chain_state
                    .handle_finality_signature(fs, gossiped);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::GetStoredFinalitySignaturesResult(fs, maybe_signatures) => {
                let outcomes = self
                    .linear_chain_state
                    .handle_cached_signatures(maybe_signatures, fs);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::IsBonded(maybe_known_signatures, new_fs, is_bonded) => {
                let outcomes = self.linear_chain_state.handle_is_bonded(
                    maybe_known_signatures,
                    new_fs,
                    is_bonded,
                );
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::Upgrade => {
                self.stop_for_upgrade = true;
                Effects::new()
            }
            Event::GotUpgradeActivationPoint(activation_point) => {
                self.linear_chain_state
                    .got_upgrade_activation_point(activation_point);
                Effects::new()
            }
        }
    }
}

/// A trait to allow an `EffectBuilder` to be replaced in testing by a mock.
#[async_trait]
pub(crate) trait EraValidatorsGetter: Copy {
    async fn get_switch_block_header_at_era_id_from_storage(
        self,
        era_id: EraId,
    ) -> Option<BlockHeader>;

    async fn get_era_validators_from_contract_runtime(
        self,
        request: EraValidatorsRequest,
    ) -> Result<EraValidators, GetEraValidatorsError>;
}

#[async_trait]
impl<REv> EraValidatorsGetter for EffectBuilder<REv>
where
    REv: From<StorageRequest> + From<ContractRuntimeRequest> + Send,
{
    async fn get_switch_block_header_at_era_id_from_storage(
        self,
        era_id: EraId,
    ) -> Option<BlockHeader> {
        self.get_switch_block_header_at_era_id_from_storage(era_id)
            .await
    }

    async fn get_era_validators_from_contract_runtime(
        self,
        request: EraValidatorsRequest,
    ) -> Result<EraValidators, GetEraValidatorsError> {
        self.get_era_validators_from_contract_runtime(request).await
    }
}

/// Returns the validator weights that should be used to check the block signatures for the given
/// block, taking into account validator-change upgrades.
pub(crate) async fn era_validator_weights_for_block<T: EraValidatorsGetter>(
    block_header: &BlockHeader,
    era_validators_getter: T,
) -> Result<(EraId, ValidatorWeights), Error> {
    let era_for_validators_retrieval = block_header.era_id().saturating_sub(1);
    let switch_block_of_previous_era = era_validators_getter
        .get_switch_block_header_at_era_id_from_storage(era_for_validators_retrieval)
        .await
        .ok_or(Error::NoSwitchBlockForEra {
            era_id: era_for_validators_retrieval,
        })?;
    if block_header.protocol_version() != switch_block_of_previous_era.protocol_version() {
        if let Some(next_validator_weights) = block_header.next_era_validator_weights() {
            let request = EraValidatorsRequest::new(
                *switch_block_of_previous_era.state_root_hash(),
                switch_block_of_previous_era.protocol_version(),
            );
            let validator_map = era_validators_getter
                .get_era_validators_from_contract_runtime(request)
                .await
                .map_err(Error::GetEraValidators)?;
            let next_era_id = block_header.next_block_era_id();
            if let Some(next_validator_weights_according_to_previous_block) =
                validator_map.get(&next_era_id)
            {
                if next_validator_weights_according_to_previous_block != next_validator_weights {
                    // The validator weights that had been assigned to next_era_id before the
                    // upgrade don't match the ones in block_header. So the validators were changed
                    // as part of the upgrade. That usually means that the ones before the upgrade
                    // cannot be trusted anymore, and we expect the new validators to sign this
                    // block.
                    info!(
                        ?switch_block_of_previous_era,
                        ?next_validator_weights_according_to_previous_block,
                        first_block_after_the_upgrade = ?block_header,
                        next_validator_weights_after_the_upgrade = ?next_validator_weights,
                        "validator map changed in upgrade"
                    );
                    return Ok((block_header.era_id(), next_validator_weights.clone()));
                }
            } else {
                return Err(Error::MissingValidatorMapEntry {
                    block_header: Box::new(switch_block_of_previous_era),
                    missing_era_id: block_header.next_block_era_id(),
                });
            }
        }
    }
    let validator_weights = switch_block_of_previous_era
        .next_era_validator_weights()
        .ok_or(Error::MissingNextEraValidators {
            height: switch_block_of_previous_era.height(),
            era_id: era_for_validators_retrieval,
        })?;
    Ok((era_for_validators_retrieval, validator_weights.clone()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::FutureExt;

    use casper_hashing::Digest;
    use casper_types::{testing::TestRng, PublicKey, SemVer, Timestamp, U512};

    use super::*;
    use crate::{
        components::consensus::EraReport,
        types::{Block, BlockHash, BlockPayload, FinalizedBlock},
    };

    const ERA_0: EraId = EraId::new(0);
    const ERA_1: EraId = EraId::new(1);
    const ERA_2: EraId = EraId::new(2);
    const ERA_3: EraId = EraId::new(3);
    const ERA_4: EraId = EraId::new(4);
    const ERA_5: EraId = EraId::new(5);
    const ERA_6: EraId = EraId::new(6);
    const VERSION_1: ProtocolVersion = ProtocolVersion::V1_0_0;
    const VERSION_2: ProtocolVersion = ProtocolVersion::new(SemVer::new(2, 0, 0));
    const VERSION_3: ProtocolVersion = ProtocolVersion::new(SemVer::new(3, 0, 0));

    fn random_validators(rng: &mut TestRng, base_weight: u64) -> ValidatorWeights {
        (0..10)
            .map(|index| (PublicKey::random(rng), U512::from(base_weight + index)))
            .collect()
    }

    #[derive(Default)]
    struct Fixture {
        // The validators and weights applicable for the given block height.
        expected_validators_for_block_height: BTreeMap<u64, ValidatorWeights>,
        // The blocks, keyed by block height.
        blocks: BTreeMap<u64, Block>,
        // The switch blocks, keyed by era ID.
        switch_blocks: BTreeMap<EraId, Block>,
        // The validators and weights as recorded at each global state root hash.
        validators_in_global_state: BTreeMap<Digest, EraValidators>,
    }

    impl Fixture {
        /// This constructs a fixture where we have a block chain as follows:
        ///
        /// Block  |     | Protocol | Next Era
        /// Height | Era | Version  | Validators
        /// ===========================================
        ///    2   |  0  |  1.0.0   | Era 1 weights (switch block)
        ///    3   |  1  |  1.0.0   | None
        ///    4   |  1  |  1.0.0   | Era 2 weights (switch block)
        ///    5   |  2  |  1.0.0   | None
        ///    6   |  2  |  1.0.0   | Era 3 weights (switch block)
        ///    7   |  3  |  2.0.0   | Era 4 weights (immediate switch block after upgrade)
        ///    8   |  4  |  2.0.0   | None
        ///    9   |  4  |  2.0.0   | Bad era 5 weights (switch block)
        ///   10   |  5  |  3.0.0   | Era 6 weights (immediate switch block after validator-change
        ///                                          upgrade where bad weights were replaced)
        ///   11   |  6  |  3.0.0   | None
        fn new() -> Self {
            let mut rng = TestRng::new();
            let era_1_validators = random_validators(&mut rng, 100);
            let era_2_validators = random_validators(&mut rng, 200);
            let era_3_validators = random_validators(&mut rng, 300);
            let era_4_validators = random_validators(&mut rng, 400);
            let original_era_5_validators = random_validators(&mut rng, 500);
            let replacement_era_5_validators = random_validators(&mut rng, 550);
            // The global_state_update_gen tool ensures we don't allow rotating validators while
            // running commit_step during the creation of the immediate switch block after a
            // validator-change upgrade.
            let era_6_validators = replacement_era_5_validators.clone();
            let era_7_validators = random_validators(&mut rng, 700);

            let mut fixture = Fixture::default();

            // Block 2: switch block at end of era 0.
            let parent_hash = BlockHash::new(Digest::hash(b"parent state root hash"));
            let parent_seed = Digest::hash(b"parent accumulated seed");
            let state_root_hash = Digest::hash(&[2]);
            let finalized_block = FinalizedBlock::new(
                BlockPayload::default(),
                Some(EraReport::default()),
                Timestamp::now(),
                ERA_0,
                2,
                era_1_validators.keys().next().unwrap().clone(),
            );
            let block_2 = Block::new(
                parent_hash,
                parent_seed,
                state_root_hash,
                finalized_block,
                Some(era_1_validators.clone()),
                VERSION_1,
            )
            .unwrap();
            fixture
                .expected_validators_for_block_height
                .insert(block_2.height(), era_1_validators.clone());
            fixture.blocks.insert(block_2.height(), block_2.clone());
            fixture
                .switch_blocks
                .insert(block_2.header().era_id(), block_2.clone());
            let validators_in_global_state: EraValidators = [
                (ERA_0, &era_1_validators),
                (ERA_1, &era_1_validators),
                (ERA_2, &era_2_validators),
            ]
            .iter()
            .map(|(era, validators)| (*era, (*validators).clone()))
            .collect();
            fixture
                .validators_in_global_state
                .insert(*block_2.state_root_hash(), validators_in_global_state);

            // Block 3: non-switch block in era 1.
            fixture.create_next_block(
                ERA_1,
                None,
                era_1_validators.keys().next().unwrap().clone(),
                VERSION_1,
                era_1_validators.clone(),
            );

            // Block 4: switch block in era 1.
            fixture.create_next_block(
                ERA_1,
                Some((era_2_validators.clone(), era_3_validators.clone())),
                era_1_validators.keys().next().unwrap().clone(),
                VERSION_1,
                era_1_validators,
            );

            // Block 5: non-switch block in era 2.
            fixture.create_next_block(
                ERA_2,
                None,
                era_2_validators.keys().next().unwrap().clone(),
                VERSION_1,
                era_2_validators.clone(),
            );

            // Block 6: switch block in era 2.
            fixture.create_next_block(
                ERA_2,
                Some((era_3_validators.clone(), era_4_validators.clone())),
                era_2_validators.keys().next().unwrap().clone(),
                VERSION_1,
                era_2_validators,
            );

            // Block 7: immediate switch block in era 3 after upgrade to version 2.0.0.
            fixture.create_next_block(
                ERA_3,
                Some((era_4_validators.clone(), original_era_5_validators.clone())),
                PublicKey::System,
                VERSION_2,
                era_3_validators,
            );

            // Block 8: non-switch block in era 4.
            fixture.create_next_block(
                ERA_4,
                None,
                era_4_validators.keys().next().unwrap().clone(),
                VERSION_2,
                era_4_validators.clone(),
            );

            // Block 9: switch block in era 4.
            fixture.create_next_block(
                ERA_4,
                Some((original_era_5_validators.clone(), original_era_5_validators)),
                era_4_validators.keys().next().unwrap().clone(),
                VERSION_2,
                era_4_validators,
            );

            // Block 10: immediate switch block in era 5 after validator-change upgrade to version
            // 3.0.0.
            fixture.create_next_block(
                ERA_5,
                Some((era_6_validators.clone(), era_7_validators)),
                PublicKey::System,
                VERSION_3,
                replacement_era_5_validators,
            );

            // Block 11: non-switch block in era 6.
            fixture.create_next_block(
                ERA_6,
                None,
                era_6_validators.keys().next().unwrap().clone(),
                VERSION_3,
                era_6_validators,
            );

            fixture
        }

        fn create_next_block(
            &mut self,
            era: EraId,
            next_two_eras_validator_weights: Option<(ValidatorWeights, ValidatorWeights)>,
            proposer: PublicKey,
            protocol_version: ProtocolVersion,
            validators_for_this_block: ValidatorWeights,
        ) {
            let last_block = self.blocks.values().last().unwrap();
            let era_report = next_two_eras_validator_weights
                .as_ref()
                .map(|_| EraReport::default());
            let finalized_block = FinalizedBlock::new(
                BlockPayload::default(),
                era_report,
                Timestamp::now(),
                era,
                last_block.height() + 1,
                proposer,
            );

            let state_root_hash = Digest::hash(&[era.value() as u8]);
            // If this is a switch block, update the validator weights to have entries for the next
            // two eras.  Otherwise, just copy the validator weights for the previous block.
            let validators_in_global_state = match next_two_eras_validator_weights.as_ref() {
                Some((weights_1, weights_2)) => {
                    let mut validators = EraValidators::new();
                    validators.insert(era + 1, weights_1.clone());
                    validators.insert(era + 2, weights_2.clone());
                    validators
                }
                None => self
                    .validators_in_global_state
                    .get(last_block.state_root_hash())
                    .unwrap()
                    .clone(),
            };
            self.validators_in_global_state
                .insert(state_root_hash, validators_in_global_state);

            let block = Block::new(
                *last_block.hash(),
                last_block.header().accumulated_seed(),
                state_root_hash,
                finalized_block,
                next_two_eras_validator_weights.map(|(weights_1, _)| weights_1),
                protocol_version,
            )
            .unwrap();

            assert!(self
                .expected_validators_for_block_height
                .insert(block.height(), validators_for_this_block)
                .is_none());
            assert!(self.blocks.insert(block.height(), block.clone()).is_none());
            if block.header().is_switch_block() {
                assert!(self
                    .switch_blocks
                    .insert(block.header().era_id(), block)
                    .is_none());
            }
        }
    }

    #[derive(Copy, Clone)]
    struct TestEffectBuilder<'a> {
        fixture: &'a Fixture,
    }

    #[async_trait]
    impl<'a> EraValidatorsGetter for TestEffectBuilder<'a> {
        async fn get_switch_block_header_at_era_id_from_storage(
            self,
            era_id: EraId,
        ) -> Option<BlockHeader> {
            self.fixture
                .switch_blocks
                .get(&era_id)
                .map(|block| block.header().clone())
        }

        async fn get_era_validators_from_contract_runtime(
            self,
            request: EraValidatorsRequest,
        ) -> Result<EraValidators, GetEraValidatorsError> {
            self.fixture
                .validators_in_global_state
                .get(&request.state_hash())
                .cloned()
                .ok_or(GetEraValidatorsError::RootNotFound)
        }
    }

    fn check_validators(fixture: &Fixture, block_height: u64, expected_era: EraId) {
        let effect_builder = TestEffectBuilder { fixture };

        let block_header = fixture
            .blocks
            .get(&block_height)
            .unwrap_or_else(|| panic!("should have block {}", block_height))
            .header();

        let (era, validator_weights) =
            era_validator_weights_for_block(block_header, effect_builder)
                .now_or_never()
                .expect("future should be ready")
                .unwrap_or_else(|error| {
                    panic!(
                        "era_validator_weights_for_block should succeed for block {}: {}",
                        block_height, error
                    )
                });

        assert_eq!(era, expected_era, "mismatch for block {}", block_height);
        assert_eq!(
            validator_weights,
            *fixture
                .expected_validators_for_block_height
                .get(&block_height)
                .unwrap_or_else(|| panic!(
                    "should have expected validators for block {}",
                    block_height
                )),
            "mismatch for block {}",
            block_height
        );
    }

    #[test]
    fn should_get_validators() {
        let fixture = Fixture::new();
        check_validators(&fixture, 2, ERA_0);
        check_validators(&fixture, 3, ERA_0);
        check_validators(&fixture, 4, ERA_0);
        check_validators(&fixture, 5, ERA_1);
        check_validators(&fixture, 6, ERA_1);
        check_validators(&fixture, 7, ERA_2);
        check_validators(&fixture, 8, ERA_3);
        check_validators(&fixture, 9, ERA_3);
        check_validators(&fixture, 10, ERA_5);
        check_validators(&fixture, 11, ERA_5);
    }
}
