use alloc::{
    collections::BTreeSet,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::{self, Display, Formatter};

use alloy_consensus::{
    constants::{EIP4844_TX_TYPE_ID, EIP7702_TX_TYPE_ID},
    transaction::SignerRecoverable,
    SignableTransaction, Transaction as AlloyTransaction, TxEip1559, TxEip2930, TxEnvelope,
    TxLegacy, TypedTransaction,
};
use alloy_eips::{
    eip2718::{Decodable2718, Encodable2718},
    eip2930::AccessList,
};
use alloy_primitives::{
    keccak256, Address as AlloyAddress, Bytes as AlloyBytes, Signature as AlloySignature,
    TxKind as AlloyTxKind, B256, U256 as AlloyU256,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use k256::ecdsa::{RecoveryId, Signature as K256Signature, VerifyingKey};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{de, Deserializer, Serializer};
use serde::{Deserialize, Serialize};

use super::{Address, EvmConfig, Hash, HASH_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Approval, AsymmetricType, Digest, PublicKey, SecretKey, Signature, TimeDiff, Timestamp, U256,
    U512,
};

const TRANSACTION_KIND_SERIALIZED_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Ethereum transaction type ID for legacy transactions.
pub const LEGACY_TRANSACTION_TYPE_ID: u8 = 0;

/// Ethereum transaction type ID for EIP-2930 access-list transactions.
pub const EIP2930_TRANSACTION_TYPE_ID: u8 = 1;

/// Ethereum transaction type ID for EIP-1559 dynamic-fee transactions.
pub const EIP1559_TRANSACTION_TYPE_ID: u8 = 2;

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
#[cfg_attr(feature = "json-schema", schemars(rename = "EvmTransactionHash"))]
pub struct TransactionHash(Digest);

impl TransactionHash {
    /// Creates a transaction hash from a raw digest.
    pub const fn new(hash: Digest) -> Self {
        TransactionHash(hash)
    }

    /// Returns a new `TransactionHash` directly initialized with the provided bytes.
    pub const fn from_raw(raw_digest: [u8; HASH_LENGTH]) -> Self {
        TransactionHash(Digest::from_raw(raw_digest))
    }

    /// Returns the wrapped inner digest.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns the wrapped hash.
    pub fn hash(self) -> Hash {
        Hash::new(self.0.value())
    }

    /// Returns the raw bytes backing this hash.
    pub fn value(self) -> [u8; HASH_LENGTH] {
        self.0.value()
    }

    /// Returns a lower-case hexadecimal string without a `0x` prefix.
    pub fn to_hex_string(self) -> String {
        base16::encode_lower(&self.0)
    }

    /// Returns a random EVM transaction hash.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransactionHash(Digest::from(rng.gen::<[u8; HASH_LENGTH]>()))
    }
}

impl AsRef<[u8]> for TransactionHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Display for TransactionHash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{}", base16::encode_lower(&self.0))
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
        Digest::from_bytes(bytes).map(|(hash, remainder)| (TransactionHash(hash), remainder))
    }
}

impl From<Digest> for TransactionHash {
    fn from(digest: Digest) -> Self {
        TransactionHash(digest)
    }
}

impl From<TransactionHash> for Digest {
    fn from(transaction_hash: TransactionHash) -> Self {
        transaction_hash.0
    }
}

/// The supported Ethereum transaction envelope kinds.
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
    /// Returns the Ethereum transaction type ID for this transaction kind.
    pub const fn type_id(self) -> u8 {
        match self {
            TransactionKind::Legacy => LEGACY_TRANSACTION_TYPE_ID,
            TransactionKind::Eip2930 => EIP2930_TRANSACTION_TYPE_ID,
            TransactionKind::Eip1559 => EIP1559_TRANSACTION_TYPE_ID,
        }
    }

    fn tag(self) -> u8 {
        self.type_id()
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

/// Errors returned while decoding or validating EVM transactions.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum TransactionError {
    /// The RLP was malformed or not a supported Ethereum envelope.
    Decode(String),
    /// EVM transactions are disabled in the active chainspec.
    Disabled,
    /// The transaction envelope type is not supported by this first-pass executor.
    UnsupportedTransactionType(u8),
    /// The transaction contains an access list, which this first-pass executor does not model.
    UnsupportedAccessList,
    /// A chain ID was required by the transaction envelope but was missing.
    MissingChainId,
    /// A gas price was required by the transaction envelope but was missing.
    MissingGasPrice,
    /// No transaction lane is available for packing EVM transactions.
    MissingTransactionLane,
    /// The transaction chain ID does not match the active chainspec EVM chain ID.
    ChainIdMismatch {
        /// Expected chainspec EVM chain ID.
        expected: u64,
        /// Actual transaction chain ID.
        actual: u64,
    },
    /// The legacy or EIP-2930 gas price is lower than the active block base fee.
    GasPriceBelowBaseFee {
        /// Transaction gas price.
        gas_price: u128,
        /// Active block base fee.
        base_fee: u128,
    },
    /// The EIP-1559 maximum fee per gas is lower than the active block base fee.
    MaxFeePerGasBelowBaseFee {
        /// Transaction maximum fee per gas.
        max_fee_per_gas: u128,
        /// Active block base fee.
        base_fee: u128,
    },
    /// The EIP-1559 maximum priority fee per gas must be zero because Casper
    /// does not prioritize transactions based on transaction gas parameters.
    NonZeroMaxPriorityFeePerGas {
        /// Transaction maximum priority fee per gas.
        max_priority_fee_per_gas: u128,
    },
    /// The transaction gas limit exceeds the configured EVM block gas limit.
    GasLimitExceedsBlockGasLimit {
        /// Transaction gas limit.
        gas_limit: u64,
        /// Configured EVM block gas limit.
        block_gas_limit: u64,
    },
    /// The transaction does not contain an EVM approval.
    MissingApproval,
    /// The transaction contains more than one approval.
    MultipleApprovals,
    /// The approval is not a secp256k1 signature and public key.
    NonSecp256k1Approval,
    /// The approval signature could not be recovered against the stored payload.
    InvalidApprovalSignature,
    /// The recovered signer address does not match the stored EVM sender.
    SenderMismatch,
    /// The reconstructed Ethereum transaction hash does not match the stored hash.
    HashMismatch,
    /// The sender address could not be recovered from the signature.
    SenderRecovery(String),
    /// Reconstructing the signed envelope produced metadata different from this transaction.
    InconsistentEnvelope,
}

impl Display for TransactionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::Decode(error) => {
                write!(formatter, "EVM transaction decode error: {error}")
            }
            TransactionError::Disabled => formatter.write_str("EVM transactions are disabled"),
            TransactionError::UnsupportedTransactionType(kind) => {
                write!(formatter, "unsupported EVM transaction type: {kind}")
            }
            TransactionError::UnsupportedAccessList => {
                formatter.write_str("unsupported EVM transaction access list")
            }
            TransactionError::MissingChainId => formatter.write_str("missing EVM chain ID"),
            TransactionError::MissingGasPrice => formatter.write_str("missing EVM gas price"),
            TransactionError::MissingTransactionLane => {
                formatter.write_str("missing EVM transaction lane")
            }
            TransactionError::ChainIdMismatch { expected, actual } => {
                write!(
                    formatter,
                    "EVM chain ID mismatch: expected {expected}, got {actual}"
                )
            }
            TransactionError::GasPriceBelowBaseFee {
                gas_price,
                base_fee,
            } => {
                write!(
                    formatter,
                    "EVM gas price {gas_price} is below base fee {base_fee}"
                )
            }
            TransactionError::MaxFeePerGasBelowBaseFee {
                max_fee_per_gas,
                base_fee,
            } => {
                write!(
                    formatter,
                    "EVM max fee per gas {max_fee_per_gas} is below base fee {base_fee}"
                )
            }
            TransactionError::NonZeroMaxPriorityFeePerGas {
                max_priority_fee_per_gas,
            } => {
                write!(
                    formatter,
                    "EVM max priority fee per gas {max_priority_fee_per_gas} must be zero"
                )
            }
            TransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit,
                block_gas_limit,
            } => {
                write!(
                    formatter,
                    "EVM gas limit {gas_limit} exceeds block gas limit {block_gas_limit}"
                )
            }
            TransactionError::MissingApproval => formatter.write_str("missing EVM approval"),
            TransactionError::MultipleApprovals => formatter.write_str("multiple EVM approvals"),
            TransactionError::NonSecp256k1Approval => {
                formatter.write_str("EVM approval must use secp256k1")
            }
            TransactionError::InvalidApprovalSignature => {
                formatter.write_str("invalid EVM approval signature")
            }
            TransactionError::SenderMismatch => {
                formatter.write_str("EVM approval signer does not match transaction sender")
            }
            TransactionError::HashMismatch => {
                formatter.write_str("EVM transaction hash does not match approval")
            }
            TransactionError::SenderRecovery(error) => {
                write!(formatter, "EVM transaction sender recovery error: {error}")
            }
            TransactionError::InconsistentEnvelope => {
                formatter.write_str("EVM transaction fields do not match signed envelope")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TransactionError {}

/// An unsigned Ethereum transaction payload plus one Ethereum-style Casper approval.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Transaction {
    timestamp: Timestamp,
    ttl: TimeDiff,
    hash: TransactionHash,
    from: Address,
    kind: TransactionKind,
    to: Option<Address>,
    nonce: u64,
    gas_limit: u64,
    // Legacy and EIP-2930 transactions use this fixed gas price.
    gas_price: Option<u128>,
    // EIP-1559 maximum total price per gas. Under current node rules this
    // remains useful as a sender cap, but accepted EIP-1559 transactions must
    // set `max_priority_fee_per_gas` to zero.
    max_fee_per_gas: u128,
    // EIP-1559 maximum proposer tip per gas. Casper currently does not
    // prioritize transactions based on transaction gas parameters, so node
    // config compliance rejects non-zero values.
    max_priority_fee_per_gas: Option<u128>,
    value: U256,
    input: Vec<u8>,
    chain_id: Option<u64>,
    approvals: BTreeSet<Approval>,
}

#[cfg(any(feature = "std", test))]
#[derive(Serialize)]
struct TransactionSerHelper<'a> {
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
    value: U256,
    input: &'a Vec<u8>,
    chain_id: Option<u64>,
    approvals: &'a BTreeSet<Approval>,
}

#[cfg(any(feature = "std", test))]
#[derive(Deserialize)]
struct TransactionDeserHelper {
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
    value: U256,
    input: Vec<u8>,
    chain_id: Option<u64>,
    approvals: BTreeSet<Approval>,
}

#[cfg(any(feature = "std", test))]
impl Serialize for Transaction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TransactionSerHelper {
            timestamp: self.timestamp,
            ttl: self.ttl,
            hash: self.hash,
            from: self.from,
            kind: self.kind,
            to: self.to,
            nonce: self.nonce,
            gas_limit: self.gas_limit,
            gas_price: self.gas_price,
            max_fee_per_gas: self.max_fee_per_gas,
            max_priority_fee_per_gas: self.max_priority_fee_per_gas,
            value: self.value,
            input: &self.input,
            chain_id: self.chain_id,
            approvals: &self.approvals,
        }
        .serialize(serializer)
    }
}

#[cfg(any(feature = "std", test))]
impl<'de> Deserialize<'de> for Transaction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let helper = TransactionDeserHelper::deserialize(deserializer)?;
        let transaction = Transaction {
            timestamp: helper.timestamp,
            ttl: helper.ttl,
            hash: helper.hash,
            from: helper.from,
            kind: helper.kind,
            to: helper.to,
            nonce: helper.nonce,
            gas_limit: helper.gas_limit,
            gas_price: helper.gas_price,
            max_fee_per_gas: helper.max_fee_per_gas,
            max_priority_fee_per_gas: helper.max_priority_fee_per_gas,
            value: helper.value,
            input: helper.input,
            chain_id: helper.chain_id,
            approvals: helper.approvals,
        };
        transaction.verify().map_err(de::Error::custom)?;
        Ok(transaction)
    }
}

impl Transaction {
    /// Decodes a signed Ethereum RLP transaction into an unsigned payload plus approval.
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
        let signature_hash = envelope.signature_hash();
        let approval = approval_from_alloy_signature(envelope.signature(), &signature_hash)?;
        let mut approvals = BTreeSet::new();
        approvals.insert(approval);

        let from = envelope
            .recover_signer()
            .map_err(|error| TransactionError::SenderRecovery(format!("{error:?}")))?;
        Ok(Transaction {
            timestamp,
            ttl,
            hash: b256_to_transaction_hash(*envelope.tx_hash()),
            from: alloy_address_to_address(from),
            kind,
            to,
            nonce: envelope.nonce(),
            gas_limit: envelope.gas_limit(),
            gas_price: envelope.gas_price(),
            max_fee_per_gas: envelope.max_fee_per_gas(),
            max_priority_fee_per_gas: envelope.max_priority_fee_per_gas(),
            value: alloy_u256_to_casper(envelope.value()),
            input: envelope.input().to_vec(),
            chain_id: envelope.chain_id(),
            approvals,
        })
    }

    /// Reconstructs the signed Ethereum envelope and validates sender/hash consistency.
    pub fn verify(&self) -> Result<(), TransactionError> {
        let signed = self.signed_envelope()?;
        if b256_to_transaction_hash(*signed.tx_hash()) != self.hash {
            return Err(TransactionError::HashMismatch);
        }
        let recovered = signed
            .recover_signer()
            .map_err(|error| TransactionError::SenderRecovery(format!("{error:?}")))?;
        if alloy_address_to_address(recovered) != self.from {
            return Err(TransactionError::SenderMismatch);
        }
        Ok(())
    }

    /// Signs the unsigned Ethereum payload with one secp256k1 approval.
    ///
    /// This recomputes the recovered EVM sender and Ethereum signed
    /// transaction hash from the new signature.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        self.try_sign(secret_key)
            .expect("EVM transactions must be signed with a valid secp256k1 key")
    }

    /// Attempts to sign the unsigned Ethereum payload with one secp256k1 approval.
    pub fn try_sign(&mut self, secret_key: &SecretKey) -> Result<(), TransactionError> {
        let SecretKey::Secp256k1(signing_key) = secret_key else {
            return Err(TransactionError::NonSecp256k1Approval);
        };
        let unsigned = self.unsigned_transaction()?;
        let signature_hash = unsigned.signature_hash();
        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(signature_hash.as_slice())
            .map_err(|_| TransactionError::InvalidApprovalSignature)?;
        let mut signature_bytes = [0u8; Signature::SECP256K1_LENGTH];
        signature_bytes.copy_from_slice(signature.to_bytes().as_slice());
        let signature = Signature::secp256k1(signature_bytes)
            .map_err(|_| TransactionError::InvalidApprovalSignature)?;
        let signer = PublicKey::from(secret_key);
        let mut approvals = BTreeSet::new();
        approvals.insert(Approval::new(signer, signature));

        let recovered_key =
            recover_verifying_key(&signature_hash, &signature_bytes, recovery_id.is_y_odd())?;
        let alloy_signature =
            AlloySignature::from_bytes_and_parity(&signature_bytes, recovery_id.is_y_odd());
        let signed = unsigned.into_envelope(alloy_signature);

        self.approvals = approvals;
        self.from = evm_address_from_verifying_key(&recovered_key);
        self.hash = b256_to_transaction_hash(*signed.tx_hash());
        self.verify()
    }

    /// Returns the raw signed Ethereum RLP bytes reconstructed from the approval.
    pub fn signed_rlp(&self) -> Result<Vec<u8>, TransactionError> {
        Ok(self.signed_envelope()?.encoded_2718())
    }

    /// Returns the raw signed Ethereum RLP bytes reconstructed from the approval.
    pub fn raw_signed_rlp(&self) -> Result<Vec<u8>, TransactionError> {
        self.signed_rlp()
    }

    /// Returns the Ethereum signing hash of the unsigned payload.
    pub fn signature_hash(&self) -> Result<Hash, TransactionError> {
        Ok(b256_to_hash(self.unsigned_transaction()?.signature_hash()))
    }

    /// Returns the bytes Ethereum signs for this unsigned payload.
    pub fn signing_payload(&self) -> Result<Vec<u8>, TransactionError> {
        Ok(self.unsigned_transaction()?.encoded_for_signing())
    }

    /// Returns the approvals attached to this transaction.
    pub fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }

    /// Returns this transaction with a replacement approval set.
    ///
    /// The stored Ethereum transaction hash is intentionally left unchanged;
    /// [`Transaction::verify`] rejects replacement approvals that do not
    /// reconstruct the same signed Ethereum transaction.
    pub fn with_approvals(mut self, approvals: BTreeSet<Approval>) -> Self {
        self.approvals = approvals;
        self
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

    /// Returns the transaction envelope kind.
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
    ///
    /// For EIP-1559 transactions, this is the sender's cap on the total gas
    /// price. Casper accepts EIP-1559 envelopes for tooling compatibility,
    /// but currently requires the priority fee to be zero because Casper does
    /// not prioritize transactions based on transaction gas parameters. Under
    /// those rules, accepted EIP-1559 transactions effectively pay the
    /// configured EVM base fee, capped by this value.
    pub fn max_fee_per_gas(&self) -> u128 {
        self.max_fee_per_gas
    }

    /// Returns the maximum priority fee per gas, if available.
    ///
    /// This is the EIP-1559 proposer-tip cap. Casper currently rejects
    /// non-zero priority fees during node config compliance because EVM
    /// transactions are packed using Casper's current transaction ordering
    /// policy, not Ethereum-style priority-fee bidding.
    pub fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.max_priority_fee_per_gas
    }

    /// Returns the amount of wei transferred by this transaction.
    pub fn value(&self) -> U256 {
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

    /// Returns the effective gas price at the supplied block base fee.
    ///
    /// Legacy and EIP-2930 transactions use their signed gas price directly.
    /// For EIP-1559, the calculation follows Ethereum's effective price
    /// formula: the lower of `max_fee_per_gas` and block base fee plus
    /// `max_priority_fee_per_gas`.
    ///
    /// The node execution path currently rejects non-zero EIP-1559 priority
    /// fees during chainspec compliance checks because Casper does not
    /// prioritize transactions based on transaction gas parameters. For
    /// accepted node transactions, the EIP-1559 effective gas price is
    /// therefore the block base fee capped by `max_fee_per_gas`.
    pub fn effective_gas_price(&self, base_fee: u64) -> u128 {
        match self.kind {
            TransactionKind::Legacy | TransactionKind::Eip2930 => {
                self.gas_price.unwrap_or(self.max_fee_per_gas)
            }
            TransactionKind::Eip1559 => {
                let max_priority_fee_per_gas = self.max_priority_fee_per_gas.unwrap_or(0);
                let priority_fee = self.max_fee_per_gas.saturating_sub(u128::from(base_fee));
                if priority_fee > max_priority_fee_per_gas {
                    u128::from(base_fee).saturating_add(max_priority_fee_per_gas)
                } else {
                    self.max_fee_per_gas
                }
            }
        }
    }

    /// Returns the fee amount for `gas_used` under the supplied EVM config.
    pub fn fee_amount(&self, gas_used: u64, evm_config: &EvmConfig) -> Option<U512> {
        U512::from(gas_used).checked_mul(U512::from(self.effective_gas_price(evm_config.base_fee)))
    }

    /// Returns the maximum fee amount this transaction can consume.
    pub fn max_fee_amount(&self, evm_config: &EvmConfig) -> Option<U512> {
        self.fee_amount(self.gas_limit, evm_config)
    }

    /// Returns the balance needed for value transfer plus the supplied fee amount.
    pub fn required_balance(&self, fee_amount: U512) -> Option<U512> {
        fee_amount.checked_add(U512::from(self.value))
    }

    /// Returns `true` if the transaction has expired at the given timestamp.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        current_instant > self.expires()
    }

    /// Returns the timestamp of when the transaction expires.
    pub fn expires(&self) -> Timestamp {
        self.timestamp + self.ttl
    }

    fn signed_envelope(&self) -> Result<TxEnvelope, TransactionError> {
        let unsigned = self.unsigned_transaction()?;
        let signature_hash = unsigned.signature_hash();
        let (signature, recovered_from) = self.approval_signature(&signature_hash)?;
        if recovered_from != self.from {
            return Err(TransactionError::SenderMismatch);
        }
        Ok(unsigned.into_envelope(signature))
    }

    fn unsigned_transaction(&self) -> Result<TypedTransaction, TransactionError> {
        let to = match self.to {
            Some(address) => AlloyTxKind::Call(to_alloy_address(address)),
            None => AlloyTxKind::Create,
        };
        let value = casper_u256_to_alloy(self.value);
        let input = AlloyBytes::from(self.input.clone());
        match self.kind {
            TransactionKind::Legacy => Ok(TypedTransaction::Legacy(TxLegacy {
                chain_id: self.chain_id,
                nonce: self.nonce,
                gas_price: self.gas_price.ok_or(TransactionError::MissingGasPrice)?,
                gas_limit: self.gas_limit,
                to,
                value,
                input,
            })),
            TransactionKind::Eip2930 => Ok(TypedTransaction::Eip2930(TxEip2930 {
                chain_id: self.chain_id.ok_or(TransactionError::MissingChainId)?,
                nonce: self.nonce,
                gas_price: self.gas_price.ok_or(TransactionError::MissingGasPrice)?,
                gas_limit: self.gas_limit,
                to,
                value,
                access_list: AccessList::default(),
                input,
            })),
            TransactionKind::Eip1559 => Ok(TypedTransaction::Eip1559(TxEip1559 {
                chain_id: self.chain_id.ok_or(TransactionError::MissingChainId)?,
                nonce: self.nonce,
                gas_limit: self.gas_limit,
                max_fee_per_gas: self.max_fee_per_gas,
                max_priority_fee_per_gas: self.max_priority_fee_per_gas.unwrap_or(0),
                to,
                value,
                access_list: AccessList::default(),
                input,
            })),
        }
    }

    fn approval_signature(
        &self,
        signature_hash: &B256,
    ) -> Result<(AlloySignature, Address), TransactionError> {
        let approval = self.single_approval()?;
        let raw_signature = secp256k1_signature_bytes(approval)?;
        let expected_signer = approval.signer();
        for y_parity in [false, true] {
            let alloy_signature = AlloySignature::from_bytes_and_parity(&raw_signature, y_parity);
            let recovered_key = recover_verifying_key(signature_hash, &raw_signature, y_parity)?;
            let recovered_public_key = public_key_from_verifying_key(&recovered_key)?;
            if &recovered_public_key == expected_signer {
                return Ok((
                    alloy_signature,
                    evm_address_from_verifying_key(&recovered_key),
                ));
            }
        }
        Err(TransactionError::InvalidApprovalSignature)
    }

    fn single_approval(&self) -> Result<&Approval, TransactionError> {
        let mut approvals = self.approvals.iter();
        let approval = approvals.next().ok_or(TransactionError::MissingApproval)?;
        if approvals.next().is_some() {
            return Err(TransactionError::MultipleApprovals);
        }
        Ok(approval)
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
        self.timestamp.serialized_length()
            + self.ttl.serialized_length()
            + self.hash.serialized_length()
            + self.from.serialized_length()
            + self.kind.serialized_length()
            + self.to.serialized_length()
            + self.nonce.serialized_length()
            + self.gas_limit.serialized_length()
            + self.gas_price.serialized_length()
            + self.max_fee_per_gas.serialized_length()
            + self.max_priority_fee_per_gas.serialized_length()
            + self.value.serialized_length()
            + Bytes::from(self.input.clone()).serialized_length()
            + self.chain_id.serialized_length()
            + self.approvals.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.timestamp.write_bytes(writer)?;
        self.ttl.write_bytes(writer)?;
        self.hash.write_bytes(writer)?;
        self.from.write_bytes(writer)?;
        self.kind.write_bytes(writer)?;
        self.to.write_bytes(writer)?;
        self.nonce.write_bytes(writer)?;
        self.gas_limit.write_bytes(writer)?;
        self.gas_price.write_bytes(writer)?;
        self.max_fee_per_gas.write_bytes(writer)?;
        self.max_priority_fee_per_gas.write_bytes(writer)?;
        self.value.write_bytes(writer)?;
        Bytes::from(self.input.clone()).write_bytes(writer)?;
        self.chain_id.write_bytes(writer)?;
        self.approvals.write_bytes(writer)
    }
}

impl FromBytes for Transaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (timestamp, remainder) = Timestamp::from_bytes(bytes)?;
        let (ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (hash, remainder) = TransactionHash::from_bytes(remainder)?;
        let (from, remainder) = Address::from_bytes(remainder)?;
        let (kind, remainder) = TransactionKind::from_bytes(remainder)?;
        let (to, remainder) = Option::<Address>::from_bytes(remainder)?;
        let (nonce, remainder) = u64::from_bytes(remainder)?;
        let (gas_limit, remainder) = u64::from_bytes(remainder)?;
        let (gas_price, remainder) = Option::<u128>::from_bytes(remainder)?;
        let (max_fee_per_gas, remainder) = u128::from_bytes(remainder)?;
        let (max_priority_fee_per_gas, remainder) = Option::<u128>::from_bytes(remainder)?;
        let (value, remainder) = U256::from_bytes(remainder)?;
        let (input, remainder) = Bytes::from_bytes(remainder)?;
        let (chain_id, remainder) = Option::<u64>::from_bytes(remainder)?;
        let (approvals, remainder) = BTreeSet::<Approval>::from_bytes(remainder)?;
        let transaction = Transaction {
            timestamp,
            ttl,
            hash,
            from,
            kind,
            to,
            nonce,
            gas_limit,
            gas_price,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            value,
            input: input.into(),
            chain_id,
            approvals,
        };
        transaction
            .verify()
            .map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((transaction, remainder))
    }
}

fn approval_from_alloy_signature(
    signature: &AlloySignature,
    signature_hash: &B256,
) -> Result<Approval, TransactionError> {
    let raw_signature = signature.as_rsy();
    let mut signature_bytes = [0u8; Signature::SECP256K1_LENGTH];
    signature_bytes.copy_from_slice(&raw_signature[..Signature::SECP256K1_LENGTH]);
    let recovered_key = recover_verifying_key(signature_hash, &signature_bytes, signature.v())?;
    let signer = public_key_from_verifying_key(&recovered_key)?;
    let signature = Signature::secp256k1(signature_bytes)
        .map_err(|_| TransactionError::InvalidApprovalSignature)?;
    Ok(Approval::new(signer, signature))
}

fn secp256k1_signature_bytes(approval: &Approval) -> Result<[u8; 64], TransactionError> {
    if !matches!(approval.signer(), PublicKey::Secp256k1(_))
        || !matches!(approval.signature(), Signature::Secp256k1(_))
    {
        return Err(TransactionError::NonSecp256k1Approval);
    }
    let signature_bytes = Vec::<u8>::from(approval.signature());
    signature_bytes
        .try_into()
        .map_err(|_| TransactionError::InvalidApprovalSignature)
}

fn recover_verifying_key(
    signature_hash: &B256,
    signature_bytes: &[u8; 64],
    y_parity: bool,
) -> Result<VerifyingKey, TransactionError> {
    let signature = K256Signature::try_from(signature_bytes.as_slice())
        .map_err(|_| TransactionError::InvalidApprovalSignature)?;
    let recovery_id = RecoveryId::new(y_parity, false);
    VerifyingKey::recover_from_prehash(signature_hash.as_slice(), &signature, recovery_id)
        .map_err(|_| TransactionError::InvalidApprovalSignature)
}

fn public_key_from_verifying_key(key: &VerifyingKey) -> Result<PublicKey, TransactionError> {
    PublicKey::secp256k1_from_bytes(key.to_encoded_point(true).as_ref())
        .map_err(|_| TransactionError::InvalidApprovalSignature)
}

fn evm_address_from_verifying_key(key: &VerifyingKey) -> Address {
    let encoded = key.to_encoded_point(false);
    let bytes = encoded.as_bytes();
    let digest = keccak256(&bytes[1..]);
    let mut address = [0u8; super::ADDRESS_LENGTH];
    address.copy_from_slice(&digest.as_slice()[HASH_LENGTH - super::ADDRESS_LENGTH..]);
    Address::new(address)
}

fn to_alloy_address(address: Address) -> AlloyAddress {
    AlloyAddress::from(address.value())
}

fn alloy_address_to_address(address: AlloyAddress) -> Address {
    let mut bytes = [0u8; super::ADDRESS_LENGTH];
    bytes.copy_from_slice(address.as_slice());
    Address::new(bytes)
}

fn b256_to_hash(hash: B256) -> Hash {
    Hash::new(hash.0)
}

fn b256_to_transaction_hash(hash: B256) -> TransactionHash {
    TransactionHash::from_raw(hash.0)
}

fn alloy_u256_to_casper(value: AlloyU256) -> U256 {
    U256::from_big_endian(&value.to_be_bytes::<32>())
}

fn casper_u256_to_alloy(value: U256) -> AlloyU256 {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    AlloyU256::from_be_slice(&bytes)
}
