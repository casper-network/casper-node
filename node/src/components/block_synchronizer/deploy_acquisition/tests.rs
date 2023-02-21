use std::collections::{BTreeMap, VecDeque};

use crate::types::{Block, Deploy};
use assert_matches::assert_matches;
use casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof;
use casper_types::{testing::TestRng, AccessRights, CLValue, StoredValue, URef};
use rand::Rng;

use super::*;

fn gen_test_deploys(rng: &mut TestRng) -> BTreeMap<DeployHash, Deploy> {
    let num_deploys = rng.gen_range(2..15);
    (0..num_deploys)
        .into_iter()
        .map(|_| {
            let deploy = Deploy::random(rng);
            (*deploy.hash(), deploy)
        })
        .collect()
}

fn gen_approvals_hashes<'a, I: Iterator<Item = &'a Deploy> + Clone>(
    rng: &mut TestRng,
    deploys_iter: I,
) -> ApprovalsHashes {
    let block = Block::random_with_deploys(rng, deploys_iter.clone());
    ApprovalsHashes::new(
        block.hash(),
        deploys_iter
            .map(|deploy| deploy.approvals_hash().unwrap())
            .collect(),
        TrieMerkleProof::new(
            URef::new([255; 32], AccessRights::NONE).into(),
            StoredValue::CLValue(CLValue::from_t(()).unwrap()),
            VecDeque::new(),
        ),
    )
}

#[test]
fn dont_apply_approvals_hashes_when_acquiring_by_id() {
    let mut rng = TestRng::new();
    let test_deploys = gen_test_deploys(&mut rng);
    let approvals_hashes = gen_approvals_hashes(&mut rng, test_deploys.values());

    let mut deploy_acquisition = DeployAcquisition::ById(Acquisition::new(
        test_deploys
            .iter()
            .map(|(deploy_hash, deploy)| {
                DeployId::new(*deploy_hash, deploy.approvals_hash().unwrap())
            })
            .collect(),
        false,
    ));

    assert_matches!(
        deploy_acquisition.apply_approvals_hashes(&approvals_hashes),
        Err(Error::AcquisitionByIdNotPossible)
    );
    assert_matches!(
        deploy_acquisition.needs_deploy().unwrap(),
        DeployIdentifier::ById(id) if test_deploys.contains_key(id.deploy_hash())
    );
}

#[test]
fn apply_approvals_on_acquisition_by_hash_creates_correct_ids() {
    let mut rng = TestRng::new();
    let test_deploys = gen_test_deploys(&mut rng);
    let mut deploy_acquisition =
        DeployAcquisition::new_by_hash(test_deploys.keys().copied().collect(), false);

    // Generate the ApprovalsHashes for all test deploys except the last one
    let approvals_hashes =
        gen_approvals_hashes(&mut rng, test_deploys.values().take(test_deploys.len() - 1));

    assert_matches!(
        deploy_acquisition.needs_deploy().unwrap(),
        DeployIdentifier::ByHash(hash) if test_deploys.contains_key(&hash)
    );
    assert!(deploy_acquisition
        .apply_approvals_hashes(&approvals_hashes)
        .is_ok());

    // Now acquisition is done by id
    assert_matches!(
        deploy_acquisition.needs_deploy().unwrap(),
        DeployIdentifier::ById(id) if test_deploys.contains_key(id.deploy_hash())
    );

    // Apply the deploys
    for (deploy_hash, deploy) in test_deploys.iter().take(test_deploys.len() - 1) {
        let acceptance = deploy_acquisition.apply_deploy(DeployId::new(
            *deploy_hash,
            deploy.approvals_hash().unwrap(),
        ));
        assert_matches!(acceptance, Some(Acceptance::NeededIt));
    }

    // The last deploy was excluded from acquisition when we applied the approvals hashes so it
    // should not be needed
    assert!(deploy_acquisition.needs_deploy().is_none());

    // Try to apply the last deploy; it should not be accepted
    let last_deploy = test_deploys.values().last().unwrap();
    let last_deploy_acceptance = deploy_acquisition.apply_deploy(DeployId::new(
        *last_deploy.hash(),
        last_deploy.approvals_hash().unwrap(),
    ));
    assert_matches!(last_deploy_acceptance, None);
}

#[test]
fn apply_approvals_hashes_after_having_already_applied_deploys() {
    let mut rng = TestRng::new();
    let test_deploys = gen_test_deploys(&mut rng);
    let mut deploy_acquisition =
        DeployAcquisition::new_by_hash(test_deploys.keys().copied().collect(), false);
    let (first_deploy_hash, first_deploy) = test_deploys.first_key_value().unwrap();

    let approvals_hashes = gen_approvals_hashes(&mut rng, test_deploys.values());

    // Apply a valid deploy that was not applied before. This should succeed.
    let acceptance = deploy_acquisition.apply_deploy(DeployId::new(
        *first_deploy_hash,
        first_deploy.approvals_hash().unwrap(),
    ));
    assert_matches!(acceptance, Some(Acceptance::NeededIt));

    // Apply approvals hashes. This should fail since we have already acquired deploys by hash.
    assert_matches!(
        deploy_acquisition.apply_approvals_hashes(&approvals_hashes),
        Err(Error::EncounteredNonVacantDeployState)
    );
}

#[test]
fn partially_applied_deploys_on_acquisition_by_hash_should_need_missing_deploys() {
    let mut rng = TestRng::new();
    let test_deploys = gen_test_deploys(&mut rng);
    let mut deploy_acquisition =
        DeployAcquisition::new_by_hash(test_deploys.keys().copied().collect(), false);

    assert_matches!(
        deploy_acquisition.needs_deploy().unwrap(),
        DeployIdentifier::ByHash(hash) if test_deploys.contains_key(&hash)
    );

    // Apply all the deploys except for the last one
    for (deploy_hash, deploy) in test_deploys.iter().take(test_deploys.len() - 1) {
        let acceptance = deploy_acquisition.apply_deploy(DeployId::new(
            *deploy_hash,
            deploy.approvals_hash().unwrap(),
        ));
        assert_matches!(acceptance, Some(Acceptance::NeededIt));
    }

    // Last deploy should be needed now
    let last_deploy = test_deploys.iter().last().unwrap().1;
    assert_matches!(
        deploy_acquisition.needs_deploy().unwrap(),
        DeployIdentifier::ByHash(hash) if *last_deploy.hash() == hash
    );

    // Apply the last deploy and check the acceptance
    let last_deploy_acceptance = deploy_acquisition.apply_deploy(DeployId::new(
        *last_deploy.hash(),
        last_deploy.approvals_hash().unwrap(),
    ));
    assert_matches!(last_deploy_acceptance, Some(Acceptance::NeededIt));

    // Try to add the last deploy again to check the acceptance
    let already_registered_acceptance = deploy_acquisition.apply_deploy(DeployId::new(
        *last_deploy.hash(),
        last_deploy.approvals_hash().unwrap(),
    ));
    assert_matches!(already_registered_acceptance, Some(Acceptance::HadIt));
}

#[test]
fn apply_unregistered_deploy_returns_no_acceptance() {
    let mut rng = TestRng::new();
    let test_deploys = gen_test_deploys(&mut rng);
    let mut deploy_acquisition =
        DeployAcquisition::new_by_hash(test_deploys.keys().copied().collect(), false);

    let unregistered_deploy = Deploy::random(&mut rng);
    let unregistered_deploy_acceptance = deploy_acquisition.apply_deploy(DeployId::new(
        *unregistered_deploy.hash(),
        unregistered_deploy.approvals_hash().unwrap(),
    ));

    // An unregistered deploy should not be accepted
    assert!(unregistered_deploy_acceptance.is_none());
    let first_deploy = test_deploys.iter().next().unwrap().1;
    assert_matches!(
        deploy_acquisition.needs_deploy().unwrap(),
        DeployIdentifier::ByHash(hash) if *first_deploy.hash() == hash
    );
}
