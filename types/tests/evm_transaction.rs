use alloy_consensus::{SignableTransaction, TxEip2930, TxEnvelope};
use alloy_eips::{
    eip2718::Encodable2718,
    eip2930::{AccessList, AccessListItem},
};
use alloy_primitives::{Address as AlloyAddress, Signature, TxKind, B256, U256};
use casper_types::{
    evm::{
        Address, Hash, Transaction, TransactionError, TransactionKind, EIP4844_TRANSACTION_TYPE_ID,
        EIP7702_TRANSACTION_TYPE_ID,
    },
    TimeDiff, Timestamp,
};
use hex_literal::hex;

const SENDER: Address = Address::new(hex!("dceea13df2f85e3a1de99a2f1c119fa6b2296a1e"));

#[test]
fn decodes_legacy_signed_rlp() {
    let transaction = decode(hex!("f86380843b9aca008252089400000000000000000000000000000000000000017b8031a0f9f5275265b6eb94b3c40777a78b78a6f2271bee070f22baf95cc373241ad424a0455f2ffb56667d638e4c5a3e859de7f0094996cffdfbe0e6b8b5025a600002c9"));

    assert_eq!(transaction.kind(), TransactionKind::Legacy);
    assert_eq!(transaction.from(), SENDER);
    assert_eq!(
        transaction.to(),
        Some(Address::new([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1
        ]))
    );
    assert_eq!(transaction.nonce(), 0);
    assert_eq!(transaction.gas_limit(), 21_000);
    assert_eq!(transaction.gas_price(), Some(1_000_000_000));
    assert_eq!(transaction.value(), word(123));
    assert_eq!(transaction.chain_id(), Some(7));
    transaction
        .verify()
        .expect("legacy transaction should verify");
}

#[test]
fn decodes_eip2930_signed_rlp() {
    let transaction = decode(hex!("01f8690701843b9aca0082c3509400000000000000000000000000000000000000028201c8821234c080a0ae81543cd30ddc7a55203a0df0d0d1182a448754e2569c32c9758db31e324cdfa0422e1fa2e2c7bf80261ccd8b4f9ac8b6c422cf0b09505d406256855156213e92"));

    assert_eq!(transaction.kind(), TransactionKind::Eip2930);
    assert_eq!(transaction.from(), SENDER);
    assert_eq!(
        transaction.to(),
        Some(Address::new([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2
        ]))
    );
    assert_eq!(transaction.nonce(), 1);
    assert_eq!(transaction.gas_limit(), 50_000);
    assert_eq!(transaction.gas_price(), Some(1_000_000_000));
    assert_eq!(transaction.value(), word(456));
    assert_eq!(transaction.input(), &[0x12, 0x34]);
    assert_eq!(transaction.chain_id(), Some(7));
    transaction
        .verify()
        .expect("EIP-2930 transaction should verify");
}

#[test]
fn decodes_eip1559_signed_rlp() {
    let transaction = decode(hex!("02f86d07028405f5e100847735940082ea6094000000000000000000000000000000000000000382031582abcdc080a0cc943bbac7dcda95bc138f08375a3be9543d34f7aecde0b386dbc0575c9cd5bc9fdfdb2712cdea574dfc83b372c490f705c2ba59421af0a84614d63024787fee"));

    assert_eq!(transaction.kind(), TransactionKind::Eip1559);
    assert_eq!(transaction.from(), SENDER);
    assert_eq!(
        transaction.to(),
        Some(Address::new([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3
        ]))
    );
    assert_eq!(transaction.nonce(), 2);
    assert_eq!(transaction.gas_limit(), 60_000);
    assert_eq!(transaction.max_fee_per_gas(), 2_000_000_000);
    assert_eq!(transaction.max_priority_fee_per_gas(), Some(100_000_000));
    assert_eq!(transaction.value(), word(789));
    assert_eq!(transaction.input(), &[0xab, 0xcd]);
    assert_eq!(transaction.chain_id(), Some(7));
    transaction
        .verify()
        .expect("EIP-1559 transaction should verify");
}

#[test]
fn unsupported_typed_transactions_are_clear_errors() {
    let timestamp = Timestamp::zero();
    let ttl = TimeDiff::from_seconds(60);

    assert_eq!(
        Transaction::from_signed_rlp(vec![EIP4844_TRANSACTION_TYPE_ID], timestamp, ttl),
        Err(TransactionError::UnsupportedTransactionType(
            EIP4844_TRANSACTION_TYPE_ID
        ))
    );
    assert_eq!(
        Transaction::from_signed_rlp(vec![EIP7702_TRANSACTION_TYPE_ID], timestamp, ttl),
        Err(TransactionError::UnsupportedTransactionType(
            EIP7702_TRANSACTION_TYPE_ID
        ))
    );
}

#[test]
fn non_empty_access_lists_are_rejected() {
    let timestamp = Timestamp::zero();
    let ttl = TimeDiff::from_seconds(60);

    assert_eq!(
        Transaction::from_signed_rlp(signed_eip2930_with_access_list(), timestamp, ttl),
        Err(TransactionError::UnsupportedAccessList)
    );
}

fn decode<const N: usize>(bytes: [u8; N]) -> Transaction {
    Transaction::from_signed_rlp(
        bytes.to_vec(),
        Timestamp::zero(),
        TimeDiff::from_seconds(60),
    )
    .expect("transaction should decode")
}

fn word(value: u64) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[24..].copy_from_slice(&value.to_be_bytes());
    Hash::new(bytes)
}

fn signed_eip2930_with_access_list() -> Vec<u8> {
    let tx = TxEip2930 {
        chain_id: 7,
        nonce: 0,
        gas_price: 1_000_000_000,
        gas_limit: 50_000,
        to: TxKind::Call(AlloyAddress::from([2u8; 20])),
        value: U256::from(456u64),
        input: vec![0x12, 0x34].into(),
        access_list: AccessList(vec![AccessListItem {
            address: AlloyAddress::from([8u8; 20]),
            storage_keys: vec![B256::from([9u8; 32])],
        }]),
    };
    let tx = tx.into_signed(Signature::test_signature());
    let envelope: TxEnvelope = tx.into();
    envelope.encoded_2718()
}
