use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::{self, Display, Formatter};

use alloy_consensus::{
    transaction::SignerRecoverable, Transaction as AlloyTransaction, TxEnvelope,
};
use alloy_eips::eip2718::{Decodable2718, EIP4844_TX_TYPE_ID, EIP7702_TX_TYPE_ID};
use alloy_primitives::{Address as AlloyAddress, TxKind as AlloyTxKind, B256};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{de, Deserializer, Serializer};
use serde::{Deserialize, Serialize};

use super::{Address, Hash, HASH_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    TimeDiff, Timestamp,
};

const TRANSACTION_KIND_SERIALIZED_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Ethereum transaction type ID for EIP-4844 blob transactions.
pub const EIP4844_TRANSACTION_TYPE_ID: u8 = EIP4844_TX_TYPE_ID;

/// Ethereum transaction type ID for EIP-7702 set-code transactions.
pub const EIP7702_TRANSACTION_TYPE_ID: u8 = EIP7702_TX_TYPE_ID;

/// A transaction hash produced by Ethereum transaction RLP hashing rules.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct TransactionHash(Hash);

impl TransactionHash {
    /// Creates a transaction hash from raw bytes.
    pub const fn new(bytes: [u8; HASH_LENGTH]) -> Self {
        TransactionHash(Hash::new(bytes))
    }

    /// Returns the wrapped hash.
    pub const fn hash(self) -> Hash {
        self.0
    }

    /// Returns the raw bytes backing this hash.
    pub const fn value(self) -> [u8; HASH_LENGTH] {
        self.0.value()
    }

    /// Returns a lower-case hexadecimal string without a `0x` prefix.
    pub fn to_hex_string(self) -> String {
        self.0.to_hex_string()
    }

    /// Returns a random EVM transaction hash.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransactionHash(Hash::new(rng.gen()))
    }
}

impl AsRef<[u8]> for TransactionHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Display for TransactionHash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl ToBytes for TransactionHash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for TransactionHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Hash::from_bytes(bytes).map(|(hash, remainder)| (TransactionHash(hash), remainder))
    }
}

/// The supported Ethereum signed transaction envelope kinds.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum TransactionKind {
    /// A legacy Ethereum transaction.
    Legacy,
    /// An EIP-2930 access-list transaction.
    Eip2930,
    /// An EIP-1559 dynamic-fee transaction.
    Eip1559,
}

impl TransactionKind {
    fn tag(self) -> u8 {
        match self {
            TransactionKind::Legacy => 0,
            TransactionKind::Eip2930 => 1,
            TransactionKind::Eip1559 => 2,
        }
    }
}

impl Display for TransactionKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionKind::Legacy => formatter.write_str("legacy"),
            TransactionKind::Eip2930 => formatter.write_str("eip2930"),
            TransactionKind::Eip1559 => formatter.write_str("eip1559"),
        }
    }
}

impl ToBytes for TransactionKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(vec![self.tag()])
    }

    fn serialized_length(&self) -> usize {
        TRANSACTION_KIND_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        Ok(())
    }
}

impl FromBytes for TransactionKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let kind = match tag {
            0 => TransactionKind::Legacy,
            1 => TransactionKind::Eip2930,
            2 => TransactionKind::Eip1559,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((kind, remainder))
    }
}

/// Errors returned while decoding or validating signed EVM transactions.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum TransactionError {
    /// The signed RLP was malformed or not a supported Ethereum envelope.
    Decode(String),
    /// The transaction envelope type is not supported by this first-pass executor.
    UnsupportedTransactionType(u8),
    /// The transaction contains an access list, which this first-pass executor does not model.
    UnsupportedAccessList,
    /// The sender address could not be recovered from the signature.
    SenderRecovery(String),
    /// Re-decoding the raw RLP produced metadata different from this transaction.
    InconsistentEnvelope,
}

impl Display for TransactionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::Decode(error) => {
                write!(formatter, "EVM transaction decode error: {error}")
            }
            TransactionError::UnsupportedTransactionType(kind) => {
                write!(formatter, "unsupported EVM transaction type: {kind}")
            }
            TransactionError::UnsupportedAccessList => {
                formatter.write_str("unsupported EVM transaction access list")
            }
            TransactionError::SenderRecovery(error) => {
                write!(formatter, "EVM transaction sender recovery error: {error}")
            }
            TransactionError::InconsistentEnvelope => {
                formatter.write_str("EVM transaction fields do not match signed RLP")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TransactionError {}

/// A decoded signed Ethereum transaction plus Casper envelope metadata.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Transaction {
    raw_signed_rlp: Vec<u8>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    hash: TransactionHash,
    from: Address,
    kind: TransactionKind,
    to: Option<Address>,
    nonce: u64,
    gas_limit: u64,
    gas_price: Option<u128>,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: Option<u128>,
    value: Hash,
    input: Vec<u8>,
    chain_id: Option<u64>,
}

#[cfg(any(feature = "std", test))]
#[derive(Serialize)]
struct TransactionSerHelper<'a> {
    raw_signed_rlp: &'a Vec<u8>,
    timestamp: Timestamp,
    ttl: TimeDiff,
}

#[cfg(any(feature = "std", test))]
#[derive(Deserialize)]
struct TransactionDeserHelper {
    raw_signed_rlp: Vec<u8>,
    timestamp: Timestamp,
    ttl: TimeDiff,
}

#[cfg(any(feature = "std", test))]
impl Serialize for Transaction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TransactionSerHelper {
            raw_signed_rlp: &self.raw_signed_rlp,
            timestamp: self.timestamp,
            ttl: self.ttl,
        }
        .serialize(serializer)
    }
}

#[cfg(any(feature = "std", test))]
impl<'de> Deserialize<'de> for Transaction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let helper = TransactionDeserHelper::deserialize(deserializer)?;
        Transaction::from_signed_rlp(helper.raw_signed_rlp, helper.timestamp, helper.ttl)
            .map_err(de::Error::custom)
    }
}

impl Transaction {
    /// Decodes a signed Ethereum RLP transaction and attaches Casper envelope metadata.
    pub fn from_signed_rlp(
        raw_signed_rlp: Vec<u8>,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Result<Self, TransactionError> {
        if matches!(raw_signed_rlp.first(), Some(&EIP4844_TRANSACTION_TYPE_ID)) {
            // EIP-4844 is proto-danksharding/blob transaction support. It
            // adds blob-carrying transactions with fields like
            // `max_fee_per_blob_gas` and `blob_versioned_hashes`, plus blob
            // gas accounting, blob base fee validation, KZG
            // commitments/proofs, and separate network representations for
            // blob sidecars. Our current transaction type and executor block
            // context only model normal EVM call/create execution, not blob
            // sidecars, blob fee markets, or block/header blob accounting.
            return Err(TransactionError::UnsupportedTransactionType(
                raw_signed_rlp[0],
            ));
        }

        if matches!(raw_signed_rlp.first(), Some(&EIP7702_TRANSACTION_TYPE_ID)) {
            // EIP-7702 lets EOAs temporarily behave like they have delegated
            // code by attaching an `authorization_list`; the protocol
            // processes those authorizations before execution and writes
            // delegation indicators like `0xef0100 || address` into account
            // code. Our current transaction type does not store an
            // authorization list, and the executor/state adapter does not
            // implement that pre-execution account-code mutation and nonce
            // logic.
            return Err(TransactionError::UnsupportedTransactionType(
                raw_signed_rlp[0],
            ));
        }

        let mut encoded = raw_signed_rlp.as_slice();
        let envelope = TxEnvelope::decode_2718(&mut encoded)
            .map_err(|error| TransactionError::Decode(format!("{error:?}")))?;
        if !encoded.is_empty() {
            return Err(TransactionError::Decode(
                "trailing bytes after transaction envelope".to_string(),
            ));
        }
        if envelope
            .access_list()
            .is_some_and(|access_list| !access_list.is_empty())
        {
            return Err(TransactionError::UnsupportedAccessList);
        }
        let from = envelope
            .recover_signer()
            .map_err(|error| TransactionError::SenderRecovery(format!("{error:?}")))?;
        let hash = b256_to_hash(*envelope.tx_hash());
        let kind = if envelope.is_legacy() {
            TransactionKind::Legacy
        } else if envelope.is_eip2930() {
            TransactionKind::Eip2930
        } else if envelope.is_eip1559() {
            TransactionKind::Eip1559
        } else {
            return Err(TransactionError::UnsupportedTransactionType(
                envelope.tx_type() as u8,
            ));
        };
        let to = match envelope.kind() {
            AlloyTxKind::Call(address) => Some(alloy_address_to_address(address)),
            AlloyTxKind::Create => None,
        };

        Ok(Transaction {
            raw_signed_rlp,
            timestamp,
            ttl,
            hash: TransactionHash(hash),
            from: alloy_address_to_address(from),
            kind,
            to,
            nonce: envelope.nonce(),
            gas_limit: envelope.gas_limit(),
            gas_price: envelope.gas_price(),
            max_fee_per_gas: envelope.max_fee_per_gas(),
            max_priority_fee_per_gas: envelope.max_priority_fee_per_gas(),
            value: Hash::new(envelope.value().to_be_bytes()),
            input: envelope.input().to_vec(),
            chain_id: envelope.chain_id(),
        })
    }

    /// Re-decodes the signed RLP and checks that all derived fields still match.
    pub fn verify(&self) -> Result<(), TransactionError> {
        let decoded =
            Transaction::from_signed_rlp(self.raw_signed_rlp.clone(), self.timestamp, self.ttl)?;
        if decoded == *self {
            Ok(())
        } else {
            Err(TransactionError::InconsistentEnvelope)
        }
    }

    /// Returns the raw signed Ethereum RLP bytes.
    pub fn raw_signed_rlp(&self) -> &[u8] {
        &self.raw_signed_rlp
    }

    /// Returns the Casper envelope timestamp.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the Casper envelope time to live.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    /// Returns the Ethereum transaction hash.
    pub fn hash(&self) -> TransactionHash {
        self.hash
    }

    /// Returns the recovered Ethereum sender address.
    pub fn from(&self) -> Address {
        self.from
    }

    /// Returns the signed transaction envelope kind.
    pub fn kind(&self) -> TransactionKind {
        self.kind
    }

    /// Returns the recipient address, or `None` for contract creation.
    pub fn to(&self) -> Option<Address> {
        self.to
    }

    /// Returns the account nonce.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Returns the transaction gas limit.
    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    /// Returns the legacy gas price, if available.
    pub fn gas_price(&self) -> Option<u128> {
        self.gas_price
    }

    /// Returns the maximum fee per gas.
    pub fn max_fee_per_gas(&self) -> u128 {
        self.max_fee_per_gas
    }

    /// Returns the maximum priority fee per gas, if available.
    pub fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.max_priority_fee_per_gas
    }

    /// Returns the transferred value as a 32-byte big-endian word.
    pub fn value(&self) -> Hash {
        self.value
    }

    /// Returns transaction input bytes.
    pub fn input(&self) -> &[u8] {
        &self.input
    }

    /// Returns the Ethereum chain ID encoded in the transaction, if present.
    pub fn chain_id(&self) -> Option<u64> {
        self.chain_id
    }

    /// Returns `true` if the transaction has expired at the given timestamp.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        current_instant > self.expires()
    }

    /// Returns the timestamp of when the transaction expires.
    pub fn expires(&self) -> Timestamp {
        self.timestamp + self.ttl
    }
}

impl Display for Transaction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EVM transaction {} from {}",
            self.hash.to_hex_string(),
            self.from
        )
    }
}

impl ToBytes for Transaction {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.raw_signed_rlp.serialized_length()
            + self.timestamp.serialized_length()
            + self.ttl.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.raw_signed_rlp.write_bytes(writer)?;
        self.timestamp.write_bytes(writer)?;
        self.ttl.write_bytes(writer)
    }
}

impl FromBytes for Transaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (raw_signed_rlp, remainder) = Vec::<u8>::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let transaction = Transaction::from_signed_rlp(raw_signed_rlp, timestamp, ttl)
            .map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((transaction, remainder))
    }
}

fn alloy_address_to_address(address: AlloyAddress) -> Address {
    let mut bytes = [0u8; super::ADDRESS_LENGTH];
    bytes.copy_from_slice(address.as_slice());
    Address::new(bytes)
}

fn b256_to_hash(hash: B256) -> Hash {
    Hash::new(hash.0)
}
