use std::collections::BTreeMap;

use casper_types::testing::TestRng;
use num_rational::Ratio;

use crate::components::consensus::tests::utils::{ALICE_PUBLIC_KEY, ALICE_SECRET_KEY};

use super::*;

#[test]
fn handle_acceptance() {
    let mut rng = TestRng::new();
    let block = Block::random(&mut rng);
    let mut builder = BlockBuilder::new(
        block.header().block_hash(),
        false,
        1,
        TimeDiff::from_seconds(1),
        TimeDiff::from_seconds(1),
    );

    let honest_peer = NodeId::random(&mut rng);
    let dishonest_peer = NodeId::random(&mut rng);

    assert!(builder
        .handle_acceptance(None, Ok(Some(Acceptance::NeededIt)))
        .is_ok());
    assert!(builder.peer_list().qualified_peers(&mut rng).is_empty());
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder
        .handle_acceptance(None, Ok(Some(Acceptance::HadIt)))
        .is_ok());
    assert!(builder.peer_list().qualified_peers(&mut rng).is_empty());
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder.handle_acceptance(None, Ok(None)).is_ok());
    assert!(builder.peer_list().qualified_peers(&mut rng).is_empty());
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder
        .handle_acceptance(Some(honest_peer), Ok(None))
        .is_ok());
    assert!(builder.peer_list().qualified_peers(&mut rng).is_empty());
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder
        .handle_acceptance(Some(honest_peer), Ok(Some(Acceptance::HadIt)))
        .is_ok());
    assert!(builder.peer_list().qualified_peers(&mut rng).is_empty());
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder
        .handle_acceptance(Some(honest_peer), Ok(Some(Acceptance::NeededIt)))
        .is_ok());
    assert!(builder
        .peer_list()
        .qualified_peers(&mut rng)
        .contains(&honest_peer));
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder
        .handle_acceptance(None, Err(BlockAcquisitionError::InvalidStateTransition))
        .is_err());
    assert!(builder
        .peer_list()
        .qualified_peers(&mut rng)
        .contains(&honest_peer));
    assert!(builder.peer_list().dishonest_peers().is_empty());

    assert!(builder
        .handle_acceptance(
            Some(dishonest_peer),
            Err(BlockAcquisitionError::InvalidStateTransition)
        )
        .is_err());
    assert!(builder
        .peer_list()
        .qualified_peers(&mut rng)
        .contains(&honest_peer));
    assert!(builder
        .peer_list()
        .dishonest_peers()
        .contains(&dishonest_peer));
}

#[test]
fn register_era_validator_weights() {
    let mut rng = TestRng::new();
    let block = Block::random(&mut rng);
    let mut builder = BlockBuilder::new(
        block.header().block_hash(),
        false,
        1,
        TimeDiff::from_seconds(1),
        TimeDiff::from_seconds(1),
    );

    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());

    builder.register_era_validator_weights(&validator_matrix);
    assert!(builder.validator_weights.is_none());

    builder.era_id = Some(EraId::from(1000));
    builder.register_era_validator_weights(&validator_matrix);
    assert!(builder.validator_weights.is_none());

    builder.era_id = Some(block.header().era_id());
    let weights = EraValidatorWeights::new(
        block.header().era_id(),
        BTreeMap::from([(ALICE_PUBLIC_KEY.clone(), 100.into())]),
        Ratio::new(1, 3),
    );
    validator_matrix.register_era_validator_weights(weights.clone());
    builder.register_era_validator_weights(&validator_matrix);
    assert_eq!(builder.validator_weights.unwrap(), weights);
}

#[test]
fn register_block_execution_enqueued() {
    let mut rng = TestRng::new();
    let block = Block::random(&mut rng);
    let mut builder = BlockBuilder::new(
        block.header().block_hash(),
        false,
        1,
        TimeDiff::from_seconds(1),
        TimeDiff::from_seconds(1),
    );
    let weights = EraValidatorWeights::new(
        block.header().era_id(),
        BTreeMap::from([(ALICE_PUBLIC_KEY.clone(), 100.into())]),
        Ratio::new(1, 3),
    );
    let mut signature_acquisition = SignatureAcquisition::new(vec![ALICE_PUBLIC_KEY.clone()]);
    let sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    assert_eq!(
        signature_acquisition.apply_signature(sig, &weights),
        Acceptance::NeededIt
    );
    builder.acquisition_state =
        BlockAcquisitionState::HaveAllDeploys(Box::new(block), signature_acquisition);

    assert_eq!(builder.execution_progress, ExecutionProgress::Idle);

    builder.register_block_execution_enqueued();
    assert_eq!(builder.execution_progress, ExecutionProgress::Started);
    assert!(!builder.is_failed());

    builder.should_fetch_execution_state = true;
    builder.register_block_execution_enqueued();
    assert!(builder.is_failed());
}

#[test]
fn register_block_executed() {
    let mut rng = TestRng::new();
    let block = Block::random(&mut rng);
    let mut builder = BlockBuilder::new(
        block.header().block_hash(),
        false,
        1,
        TimeDiff::from_seconds(1),
        TimeDiff::from_seconds(1),
    );
    let weights = EraValidatorWeights::new(
        block.header().era_id(),
        BTreeMap::from([(ALICE_PUBLIC_KEY.clone(), 100.into())]),
        Ratio::new(1, 3),
    );
    let mut signature_acquisition = SignatureAcquisition::new(vec![ALICE_PUBLIC_KEY.clone()]);
    let sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    assert_eq!(
        signature_acquisition.apply_signature(sig, &weights),
        Acceptance::NeededIt
    );
    builder.acquisition_state =
        BlockAcquisitionState::HaveStrictFinalitySignatures(Box::new(block), signature_acquisition);
    builder.execution_progress = ExecutionProgress::Started;

    builder.register_block_executed();
    assert_eq!(builder.execution_progress, ExecutionProgress::Done);
    assert!(!builder.is_failed());

    builder.should_fetch_execution_state = true;
    builder.register_block_executed();
    assert!(builder.is_failed());
}
