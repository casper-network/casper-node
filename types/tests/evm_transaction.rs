use std::collections::BTreeSet;

use alloy_consensus::{
    crypto::secp256k1, transaction::SignerRecoverable, SignableTransaction, TxEip1559, TxEip2930,
    TxEip7702, TxEnvelope, TxLegacy,
};
use alloy_eips::{
    eip2718::Encodable2718,
    eip2930::{AccessList, AccessListItem},
    eip7702::{
        Authorization as AlloyAuthorization, SignedAuthorization as AlloySignedAuthorization,
    },
};
use alloy_primitives::{Address as AlloyAddress, Signature, TxKind, B256, U256 as AlloyU256};
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    evm::{Address, Hash, EIP4844_TRANSACTION_TYPE_ID},
    Approval, ApprovalsHash, Digest, EvmTransaction, EvmTransactionError, EvmTransactionHash,
    EvmTransactionKind, PublicKey, SecretKey, TimeDiff, Timestamp,
    Transaction as CasperTransaction, TransactionHash, U256,
};

const SIGNING_SECRET: [u8; 32] = [7; 32];
const AUTHORIZATION_SECRET: [u8; 32] = [8; 32];

#[test]
fn decodes_legacy_signed_rlp() {
    let signed_transaction = signed_legacy_transaction();
    let transaction = decode(signed_transaction.raw_rlp.clone());

    assert_eq!(transaction.kind(), EvmTransactionKind::Legacy);
    assert_eq!(transaction.from(), signed_transaction.sender);
    assert_eq!(transaction.to(), Some(address(1)));
    assert_eq!(transaction.nonce(), 0);
    assert_eq!(transaction.gas_limit(), 21_000);
    assert_eq!(transaction.gas_price(), Some(1_000_000_000));
    assert_eq!(transaction.value(), U256::from(123u64));
    assert_eq!(transaction.chain_id(), Some(7));
    transaction
        .verify()
        .expect("legacy transaction should verify");
    assert_eq!(transaction.approvals().len(), 1);
    assert_eq!(
        transaction.signed_rlp().unwrap(),
        signed_transaction.raw_rlp
    );
}

#[test]
fn decodes_eip2930_signed_rlp() {
    let signed_transaction = signed_eip2930_transaction();
    let transaction = decode(signed_transaction.raw_rlp);

    assert_eq!(transaction.kind(), EvmTransactionKind::Eip2930);
    assert_eq!(transaction.from(), signed_transaction.sender);
    assert_eq!(transaction.to(), Some(address(2)));
    assert_eq!(transaction.nonce(), 1);
    assert_eq!(transaction.gas_limit(), 50_000);
    assert_eq!(transaction.gas_price(), Some(1_000_000_000));
    assert_eq!(transaction.value(), U256::from(456u64));
    assert_eq!(transaction.input(), &[0x12, 0x34]);
    assert_eq!(transaction.chain_id(), Some(7));
    transaction
        .verify()
        .expect("EIP-2930 transaction should verify");
}

#[test]
fn decodes_eip1559_signed_rlp() {
    let signed_transaction = signed_eip1559_transaction();
    let transaction = decode(signed_transaction.raw_rlp);

    assert_eq!(transaction.kind(), EvmTransactionKind::Eip1559);
    assert_eq!(transaction.from(), signed_transaction.sender);
    assert_eq!(transaction.to(), Some(address(3)));
    assert_eq!(transaction.nonce(), 2);
    assert_eq!(transaction.gas_limit(), 60_000);
    assert_eq!(transaction.max_fee_per_gas(), 2_000_000_000);
    assert_eq!(transaction.max_priority_fee_per_gas(), Some(100_000_000));
    assert_eq!(transaction.value(), U256::from(789u64));
    assert_eq!(transaction.input(), &[0xab, 0xcd]);
    assert_eq!(transaction.chain_id(), Some(7));
    transaction
        .verify()
        .expect("EIP-1559 transaction should verify");
}

#[test]
fn decodes_eip7702_signed_rlp() {
    let signed_transaction = signed_eip7702_transaction();
    let transaction = decode(signed_transaction.raw_rlp.clone());

    assert_eq!(transaction.kind(), EvmTransactionKind::Eip7702);
    assert_eq!(transaction.from(), signed_transaction.sender);
    assert_eq!(transaction.to(), Some(address(4)));
    assert_eq!(transaction.nonce(), 3);
    assert_eq!(transaction.gas_limit(), 70_000);
    assert_eq!(transaction.max_fee_per_gas(), 2_000_000_000);
    assert_eq!(transaction.max_priority_fee_per_gas(), Some(0));
    assert_eq!(transaction.value(), U256::from(987u64));
    assert_eq!(transaction.input(), &[0xde, 0xad]);
    assert_eq!(transaction.chain_id(), Some(7));
    assert_eq!(transaction.authorization_list().len(), 1);

    let expected = &signed_transaction.authorization_list[0];
    let actual = &transaction.authorization_list()[0];
    assert_eq!(actual.chain_id, alloy_u256_to_casper(*expected.chain_id()));
    assert_eq!(
        actual.address,
        alloy_address_to_address(*expected.address())
    );
    assert_eq!(actual.nonce, expected.nonce());
    assert_eq!(actual.y_parity, expected.y_parity());
    assert_eq!(actual.r, alloy_u256_to_casper(expected.r()));
    assert_eq!(actual.s, alloy_u256_to_casper(expected.s()));

    transaction
        .verify()
        .expect("EIP-7702 transaction should verify");
    transaction
        .signature_hash()
        .expect("EIP-7702 signing hash should be available");
    assert_eq!(
        transaction.signed_rlp().unwrap(),
        signed_transaction.raw_rlp
    );
    bytesrepr_roundtrip(&transaction);
}

#[test]
fn unsupported_typed_transactions_are_clear_errors() {
    let timestamp = Timestamp::zero();
    let ttl = TimeDiff::from_seconds(60);

    assert_eq!(
        EvmTransaction::from_signed_rlp(vec![EIP4844_TRANSACTION_TYPE_ID], timestamp, ttl),
        Err(EvmTransactionError::UnsupportedTransactionType(
            EIP4844_TRANSACTION_TYPE_ID
        ))
    );
}

#[test]
fn non_empty_access_lists_are_rejected() {
    let timestamp = Timestamp::zero();
    let ttl = TimeDiff::from_seconds(60);

    assert_eq!(
        EvmTransaction::from_signed_rlp(signed_eip2930_with_access_list(), timestamp, ttl),
        Err(EvmTransactionError::UnsupportedAccessList)
    );
    assert_eq!(
        EvmTransaction::from_signed_rlp(signed_eip7702_with_access_list(), timestamp, ttl),
        Err(EvmTransactionError::UnsupportedAccessList)
    );
}

#[test]
fn empty_eip7702_authorization_lists_are_rejected() {
    assert_eq!(
        EvmTransaction::from_signed_rlp(
            signed_eip7702_with_authorization_list(Vec::new(), AccessList::default()),
            Timestamp::zero(),
            TimeDiff::from_seconds(60),
        ),
        Err(EvmTransactionError::EmptyAuthorizationList)
    );
}

#[test]
fn approval_backed_transaction_identity_uses_evm_approval() {
    let evm_transaction = decode(signed_eip1559_transaction().raw_rlp);
    let transaction = CasperTransaction::from(evm_transaction.clone());
    let approvals_hash = ApprovalsHash::compute(evm_transaction.approvals()).unwrap();

    assert_eq!(transaction.approvals(), evm_transaction.approvals().clone());
    assert_eq!(
        transaction.compute_approvals_hash().unwrap(),
        approvals_hash
    );
    assert_eq!(transaction.compute_id().approvals_hash(), approvals_hash);
    assert_eq!(
        transaction.compute_id().transaction_hash(),
        TransactionHash::from(evm_transaction.hash())
    );
}

#[test]
fn evm_approvals_are_not_replaced_by_finalized_approvals() {
    let evm_transaction = decode(signed_eip1559_transaction().raw_rlp);
    let transaction = CasperTransaction::from(evm_transaction.clone());
    let secret_key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    let replacement_approval =
        Approval::create(&TransactionHash::from(evm_transaction.hash()), &secret_key);

    assert_eq!(
        transaction
            .with_approvals(BTreeSet::from([replacement_approval]))
            .approvals(),
        evm_transaction.approvals().clone()
    );
}

#[test]
fn evm_transaction_sign_replaces_approval_and_recomputes_identity() {
    let mut transaction = CasperTransaction::from(decode(signed_legacy_transaction().raw_rlp));
    let old_hash = transaction.hash();
    let new_secret_key = secp_secret_key([1; SecretKey::SECP256K1_LENGTH]);
    let expected_signer = PublicKey::from(&new_secret_key);

    transaction.sign(&new_secret_key);

    let CasperTransaction::Evm(evm_transaction) = transaction else {
        panic!("expected EVM transaction");
    };
    assert_eq!(evm_transaction.approvals().len(), 1);
    assert_eq!(
        evm_transaction.approvals().iter().next().unwrap().signer(),
        &expected_signer
    );
    assert_ne!(TransactionHash::from(evm_transaction.hash()), old_hash);
    evm_transaction
        .verify()
        .expect("signed transaction should verify");

    let decoded = decode(evm_transaction.signed_rlp().unwrap());
    assert_eq!(decoded.hash(), evm_transaction.hash());
    assert_eq!(decoded.from(), evm_transaction.from());
    assert_eq!(decoded.approvals(), evm_transaction.approvals());
}

#[test]
fn evm_eip7702_transaction_sign_preserves_authorizations() {
    let mut transaction = CasperTransaction::from(decode(signed_eip7702_transaction().raw_rlp));
    let CasperTransaction::Evm(evm_transaction) = &transaction else {
        panic!("expected EVM transaction");
    };
    let authorization_list = evm_transaction.authorization_list().to_vec();
    let new_secret_key = secp_secret_key([1; SecretKey::SECP256K1_LENGTH]);

    transaction.sign(&new_secret_key);

    let CasperTransaction::Evm(evm_transaction) = transaction else {
        panic!("expected EVM transaction");
    };
    assert_eq!(evm_transaction.authorization_list(), authorization_list);
    evm_transaction
        .verify()
        .expect("signed EIP-7702 transaction should verify");

    let decoded = decode(evm_transaction.signed_rlp().unwrap());
    assert_eq!(decoded.kind(), EvmTransactionKind::Eip7702);
    assert_eq!(decoded.authorization_list(), authorization_list);
    assert_eq!(decoded.from(), evm_transaction.from());
}

#[test]
#[should_panic(expected = "EVM transactions must be signed with a valid secp256k1 key")]
fn evm_transaction_sign_rejects_non_secp256k1_keys() {
    let mut transaction = CasperTransaction::from(decode(signed_legacy_transaction().raw_rlp));
    transaction.sign(&ed_secret_key([42; SecretKey::ED25519_LENGTH]));
}

#[test]
fn evm_approval_verification_rejects_bad_approval_sets() {
    let transaction = decode(signed_legacy_transaction().raw_rlp);
    assert_eq!(
        transaction.clone().with_approvals(BTreeSet::new()).verify(),
        Err(EvmTransactionError::MissingApproval)
    );

    let mut multiple_approvals = transaction.approvals().clone();
    multiple_approvals.insert(Approval::create(
        &TransactionHash::from(transaction.hash()),
        &secp_secret_key([1; SecretKey::SECP256K1_LENGTH]),
    ));
    assert_eq!(
        transaction
            .clone()
            .with_approvals(multiple_approvals)
            .verify(),
        Err(EvmTransactionError::MultipleApprovals)
    );

    let non_secp_approval = Approval::create(
        &TransactionHash::from(transaction.hash()),
        &ed_secret_key([42; SecretKey::ED25519_LENGTH]),
    );
    assert_eq!(
        transaction
            .clone()
            .with_approvals(BTreeSet::from([non_secp_approval]))
            .verify(),
        Err(EvmTransactionError::NonSecp256k1Approval)
    );
}

#[test]
fn evm_hashes_round_trip_raw_digest_bytes() {
    let raw = [0x42; 32];
    let hash = Hash::new(raw);
    assert_eq!(hash.value(), raw);
    assert_eq!(hash.as_bytes(), &raw);
    bytesrepr_roundtrip(&hash);
    let serialized = serde_json::to_string(&hash).unwrap();
    let deserialized = serde_json::from_str::<Hash>(&serialized).unwrap();
    assert_eq!(deserialized, hash);

    let digest = Digest::from_raw(raw);
    let transaction_hash = EvmTransactionHash::new(digest);
    assert_eq!(transaction_hash.inner(), &digest);
    assert_eq!(transaction_hash.value(), raw);
    assert_eq!(Digest::from(transaction_hash), digest);
    bytesrepr_roundtrip(&transaction_hash);
    let serialized = serde_json::to_string(&transaction_hash).unwrap();
    let deserialized = serde_json::from_str::<EvmTransactionHash>(&serialized).unwrap();
    assert_eq!(deserialized, transaction_hash);
}

struct SignedTransaction {
    raw_rlp: Vec<u8>,
    sender: Address,
    authorization_list: Vec<AlloySignedAuthorization>,
}

fn decode(bytes: Vec<u8>) -> EvmTransaction {
    EvmTransaction::from_signed_rlp(bytes, Timestamp::zero(), TimeDiff::from_seconds(60))
        .expect("transaction should decode")
}

fn bytesrepr_roundtrip<T>(value: &T)
where
    T: ToBytes + FromBytes + PartialEq + std::fmt::Debug,
{
    let bytes = value.to_bytes().expect("value should serialize");
    let (decoded, remainder) = T::from_bytes(&bytes).expect("value should deserialize");
    assert!(remainder.is_empty());
    assert_eq!(&decoded, value);
}

fn signed_legacy_transaction() -> SignedTransaction {
    let tx = TxLegacy {
        chain_id: Some(7),
        nonce: 0,
        gas_price: 1_000_000_000,
        gas_limit: 21_000,
        to: TxKind::Call(alloy_address(1)),
        value: AlloyU256::from(123u64),
        input: Vec::new().into(),
    };
    let signature = sign_transaction(&tx);
    signed_transaction(tx.into_signed(signature).into())
}

fn signed_eip2930_transaction() -> SignedTransaction {
    let tx = TxEip2930 {
        chain_id: 7,
        nonce: 1,
        gas_price: 1_000_000_000,
        gas_limit: 50_000,
        to: TxKind::Call(alloy_address(2)),
        value: AlloyU256::from(456u64),
        input: vec![0x12, 0x34].into(),
        access_list: AccessList::default(),
    };
    let signature = sign_transaction(&tx);
    signed_transaction(tx.into_signed(signature).into())
}

fn signed_eip1559_transaction() -> SignedTransaction {
    let tx = TxEip1559 {
        chain_id: 7,
        nonce: 2,
        gas_limit: 60_000,
        max_fee_per_gas: 2_000_000_000,
        max_priority_fee_per_gas: 100_000_000,
        to: TxKind::Call(alloy_address(3)),
        value: AlloyU256::from(789u64),
        access_list: AccessList::default(),
        input: vec![0xab, 0xcd].into(),
    };
    let signature = sign_transaction(&tx);
    signed_transaction(tx.into_signed(signature).into())
}

fn signed_eip7702_transaction() -> SignedTransaction {
    signed_transaction(signed_eip7702_envelope(
        vec![signed_authorization(alloy_address(9), 4)],
        AccessList::default(),
    ))
}

fn signed_eip7702_with_authorization_list(
    authorization_list: Vec<AlloySignedAuthorization>,
    access_list: AccessList,
) -> Vec<u8> {
    signed_eip7702_envelope(authorization_list, access_list).encoded_2718()
}

fn signed_eip7702_with_access_list() -> Vec<u8> {
    signed_eip7702_with_authorization_list(
        vec![signed_authorization(alloy_address(9), 4)],
        AccessList(vec![AccessListItem {
            address: alloy_address(8),
            storage_keys: vec![B256::from([9u8; 32])],
        }]),
    )
}

fn signed_eip7702_envelope(
    authorization_list: Vec<AlloySignedAuthorization>,
    access_list: AccessList,
) -> TxEnvelope {
    let tx = TxEip7702 {
        chain_id: 7,
        nonce: 3,
        gas_limit: 70_000,
        max_fee_per_gas: 2_000_000_000,
        max_priority_fee_per_gas: 0,
        to: alloy_address(4),
        value: AlloyU256::from(987u64),
        access_list,
        authorization_list,
        input: vec![0xde, 0xad].into(),
    };
    let signature = sign_transaction(&tx);
    tx.into_signed(signature).into()
}

fn signed_authorization(delegate_address: AlloyAddress, nonce: u64) -> AlloySignedAuthorization {
    let authorization = AlloyAuthorization {
        chain_id: AlloyU256::from(7),
        address: delegate_address,
        nonce,
    };
    let signature = secp256k1::sign_message(
        B256::from(AUTHORIZATION_SECRET),
        authorization.signature_hash(),
    )
    .expect("authorization signing should succeed");
    authorization.into_signed(signature)
}

fn signed_transaction(envelope: TxEnvelope) -> SignedTransaction {
    let sender = alloy_address_to_address(
        envelope
            .recover_signer()
            .expect("signed transaction should recover sender"),
    );
    SignedTransaction {
        raw_rlp: envelope.encoded_2718(),
        sender,
        authorization_list: envelope
            .as_eip7702()
            .map(|transaction| transaction.tx().authorization_list.clone())
            .unwrap_or_default(),
    }
}

fn sign_transaction<T: SignableTransaction<Signature>>(tx: &T) -> Signature {
    secp256k1::sign_message(B256::from(SIGNING_SECRET), tx.signature_hash())
        .expect("transaction signing should succeed")
}

fn secp_secret_key(bytes: [u8; SecretKey::SECP256K1_LENGTH]) -> SecretKey {
    SecretKey::secp256k1_from_bytes(bytes).expect("secp256k1 secret key should be valid")
}

fn ed_secret_key(bytes: [u8; SecretKey::ED25519_LENGTH]) -> SecretKey {
    SecretKey::ed25519_from_bytes(bytes).expect("ed25519 secret key should be valid")
}

fn address(value: u8) -> Address {
    let mut bytes = [0; 20];
    bytes[19] = value;
    Address::new(bytes)
}

fn alloy_address(value: u8) -> AlloyAddress {
    AlloyAddress::from(address(value).value())
}

fn alloy_address_to_address(address: AlloyAddress) -> Address {
    Address::new(address.into_array())
}

fn alloy_u256_to_casper(value: AlloyU256) -> U256 {
    U256::from_big_endian(&value.to_be_bytes::<32>())
}

fn signed_eip2930_with_access_list() -> Vec<u8> {
    let tx = TxEip2930 {
        chain_id: 7,
        nonce: 0,
        gas_price: 1_000_000_000,
        gas_limit: 50_000,
        to: TxKind::Call(alloy_address(2)),
        value: AlloyU256::from(456u64),
        input: vec![0x12, 0x34].into(),
        access_list: AccessList(vec![AccessListItem {
            address: alloy_address(8),
            storage_keys: vec![B256::from([9u8; 32])],
        }]),
    };
    let tx = tx.into_signed(Signature::test_signature());
    let envelope: TxEnvelope = tx.into();
    envelope.encoded_2718()
}
