use once_cell::sync::Lazy;

use casper_engine_test_support::{
    LmdbWasmTestBuilder, TransferRequestBuilder, LOCAL_GENESIS_REQUEST,
};
use casper_storage::{
    data_access_layer::BalanceIdentifier,
    tracking_copy::{self, ValidationError},
};
use casper_types::{
    account::AccountHash, AccessRights, Digest, Key, ProtocolVersion, PublicKey, SecretKey, URef,
    U512,
};

static ALICE_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ALICE_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ALICE_KEY));

static TRANSFER_AMOUNT_1: Lazy<U512> = Lazy::new(|| U512::from(100_000_000));

#[ignore]
#[test]
fn get_balance_should_work() {
    let protocol_version = ProtocolVersion::V2_0_0;
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let block_time = 1_000_000;
    let transfer_request = TransferRequestBuilder::new(*TRANSFER_AMOUNT_1, *ALICE_ADDR)
        .with_block_time(block_time)
        .build();

    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

    let alice_account = builder
        .get_entity_by_account_hash(*ALICE_ADDR)
        .expect("should have Alice's account");

    let alice_main_purse = alice_account.main_purse();

    let alice_balance_result = builder.get_purse_balance_result_with_proofs(
        protocol_version,
        BalanceIdentifier::Purse(alice_main_purse),
    );

    let alice_balance = alice_balance_result
        .available_balance()
        .cloned()
        .expect("should have motes");

    assert_eq!(alice_balance, *TRANSFER_AMOUNT_1);

    let state_root_hash = builder.get_post_state_hash();

    let proofs_result = alice_balance_result
        .proofs_result()
        .expect("should have proofs result");
    let balance_proof = proofs_result
        .total_balance_proof()
        .expect("should have proofs")
        .clone();

    assert!(tracking_copy::validate_balance_proof(
        &state_root_hash,
        &balance_proof,
        alice_main_purse.into(),
        &alice_balance,
    )
    .is_ok());

    let bogus_key = Key::Hash([1u8; 32]);
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &state_root_hash,
            &balance_proof,
            bogus_key.to_owned(),
            &alice_balance,
        ),
        Err(ValidationError::KeyIsNotAURef(bogus_key))
    );

    let bogus_uref: Key = Key::URef(URef::new([3u8; 32], AccessRights::READ_ADD_WRITE));
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &state_root_hash,
            &balance_proof,
            bogus_uref,
            &alice_balance,
        ),
        Err(ValidationError::UnexpectedKey)
    );

    let bogus_hash = Digest::hash([5u8; 32]);
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &bogus_hash,
            &balance_proof,
            alice_main_purse.into(),
            &alice_balance,
        ),
        Err(ValidationError::InvalidProofHash)
    );

    let bogus_motes = U512::from(1337);
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &state_root_hash,
            &balance_proof,
            alice_main_purse.into(),
            &bogus_motes,
        ),
        Err(ValidationError::UnexpectedValue)
    );
}

#[ignore]
#[test]
fn get_balance_using_public_key_should_work() {
    let protocol_version = ProtocolVersion::V2_0_0;
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let block_time = 1_000_000;
    let transfer_request = TransferRequestBuilder::new(*TRANSFER_AMOUNT_1, *ALICE_ADDR)
        .with_block_time(block_time)
        .build();

    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

    let alice_account = builder
        .get_entity_by_account_hash(*ALICE_ADDR)
        .expect("should have Alice's account");

    let alice_main_purse = alice_account.main_purse();

    let alice_balance_result =
        builder.get_public_key_balance_result_with_proofs(protocol_version, ALICE_KEY.clone());

    let alice_balance = alice_balance_result
        .available_balance()
        .cloned()
        .expect("should have motes");

    assert_eq!(alice_balance, *TRANSFER_AMOUNT_1);

    let state_root_hash = builder.get_post_state_hash();

    let proofs_result = alice_balance_result
        .proofs_result()
        .expect("should have proofs result");
    let balance_proof = proofs_result
        .total_balance_proof()
        .expect("should have proofs")
        .clone();

    assert!(tracking_copy::validate_balance_proof(
        &state_root_hash,
        &balance_proof,
        alice_main_purse.into(),
        &alice_balance,
    )
    .is_ok());

    let bogus_key = Key::Hash([1u8; 32]);
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &state_root_hash,
            &balance_proof,
            bogus_key.to_owned(),
            &alice_balance,
        ),
        Err(ValidationError::KeyIsNotAURef(bogus_key))
    );

    let bogus_uref: Key = Key::URef(URef::new([3u8; 32], AccessRights::READ_ADD_WRITE));
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &state_root_hash,
            &balance_proof,
            bogus_uref,
            &alice_balance,
        ),
        Err(ValidationError::UnexpectedKey)
    );

    let bogus_hash = Digest::hash([5u8; 32]);
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &bogus_hash,
            &balance_proof,
            alice_main_purse.into(),
            &alice_balance,
        ),
        Err(ValidationError::InvalidProofHash)
    );

    let bogus_motes = U512::from(1337);
    assert_eq!(
        tracking_copy::validate_balance_proof(
            &state_root_hash,
            &balance_proof,
            alice_main_purse.into(),
            &bogus_motes,
        ),
        Err(ValidationError::UnexpectedValue)
    );
}
