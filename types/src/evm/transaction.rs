use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::{self, Display, Formatter};

use alloy_consensus::{
    constants::{EIP4844_TX_TYPE_ID, EIP7702_TX_TYPE_ID},
    transaction::SignerRecoverable,
    SignableTransaction, Transaction as AlloyTransaction, TxEip1559, TxEip2930, TxEip7702,
    TxEnvelope, TxLegacy, TypedTransaction,
};
use alloy_eips::{
    eip2718::{Decodable2718, Encodable2718},
    eip2930::AccessList,
    eip7702::{
        Authorization as AlloyAuthorization, SignedAuthorization as AlloyAuthorizationListItem,
    },
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
    transaction::serialization::{
        CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder,
    },
    Approval, ApprovalsHash, AsymmetricType, Digest, InitiatorAddr, PublicKey, SecretKey,
    Signature, TimeDiff, Timestamp, U256, U512,
};

const TRANSACTION_KIND_SERIALIZED_LENGTH: usize = U8_SERIALIZED_LENGTH;
const EVM_TRANSACTION_MAX_CURRENT_FIELDS: u32 = 16;

const TIMESTAMP_FIELD_INDEX: u16 = 0;
const TTL_FIELD_INDEX: u16 = 1;
const KIND_FIELD_INDEX: u16 = 2;

// Field indices after KIND_FIELD_INDEX are interpreted within the selected
// EVM transaction kind. Keeping kind-specific payloads separate lets future
// transaction types add fields without changing the layout of older kinds.
const HASH_FIELD_INDEX: u16 = 3;
const FROM_FIELD_INDEX: u16 = 4;
const TO_FIELD_INDEX: u16 = 5;
const NONCE_FIELD_INDEX: u16 = 6;
const GAS_LIMIT_FIELD_INDEX: u16 = 7;

const LEGACY_GAS_PRICE_FIELD_INDEX: u16 = 8;
const LEGACY_VALUE_FIELD_INDEX: u16 = 9;
const LEGACY_INPUT_FIELD_INDEX: u16 = 10;
const LEGACY_CHAIN_ID_FIELD_INDEX: u16 = 11;
const LEGACY_APPROVAL_FIELD_INDEX: u16 = 12;

const DYNAMIC_MAX_FEE_PER_GAS_FIELD_INDEX: u16 = 8;
const DYNAMIC_MAX_PRIORITY_FEE_PER_GAS_FIELD_INDEX: u16 = 9;
const DYNAMIC_VALUE_FIELD_INDEX: u16 = 10;
const DYNAMIC_INPUT_FIELD_INDEX: u16 = 11;
const DYNAMIC_CHAIN_ID_FIELD_INDEX: u16 = 12;
const DYNAMIC_APPROVAL_FIELD_INDEX: u16 = 13;

const EIP7702_AUTHORIZATION_LIST_FIELD_INDEX: u16 = 13;
const EIP7702_APPROVAL_FIELD_INDEX: u16 = 14;
const INITIATOR_ADDR_FIELD_INDEX: u16 = 15;

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
pub struct EvmTransactionHash(Digest);

impl EvmTransactionHash {
    /// Creates a transaction hash from a raw digest.
    pub const fn new(hash: Digest) -> Self {
        EvmTransactionHash(hash)
    }

    /// Returns a new `EvmTransactionHash` directly initialized with the provided bytes.
    pub const fn from_raw(raw_digest: [u8; HASH_LENGTH]) -> Self {
        EvmTransactionHash(Digest::from_raw(raw_digest))
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
        EvmTransactionHash(Digest::from(rng.gen::<[u8; HASH_LENGTH]>()))
    }
}

impl AsRef<[u8]> for EvmTransactionHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Display for EvmTransactionHash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{}", base16::encode_lower(&self.0))
    }
}

impl ToBytes for EvmTransactionHash {
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

impl FromBytes for EvmTransactionHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(hash, remainder)| (EvmTransactionHash(hash), remainder))
    }
}

impl From<Digest> for EvmTransactionHash {
    fn from(digest: Digest) -> Self {
        EvmTransactionHash(digest)
    }
}

impl From<EvmTransactionHash> for Digest {
    fn from(transaction_hash: EvmTransactionHash) -> Self {
        transaction_hash.0
    }
}

/// The supported Ethereum transaction envelope kinds.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EvmTransactionKind {
    /// A legacy Ethereum transaction.
    Legacy,
    /// An EIP-2930 access-list transaction.
    Eip2930,
    /// An EIP-1559 dynamic-fee transaction.
    Eip1559,
    /// An EIP-7702 set-code transaction.
    Eip7702,
}

impl EvmTransactionKind {
    /// Returns the Ethereum transaction type ID for this transaction kind.
    pub const fn type_id(self) -> u8 {
        match self {
            EvmTransactionKind::Legacy => LEGACY_TRANSACTION_TYPE_ID,
            EvmTransactionKind::Eip2930 => EIP2930_TRANSACTION_TYPE_ID,
            EvmTransactionKind::Eip1559 => EIP1559_TRANSACTION_TYPE_ID,
            EvmTransactionKind::Eip7702 => EIP7702_TRANSACTION_TYPE_ID,
        }
    }

    fn tag(self) -> u8 {
        self.type_id()
    }
}

impl Display for EvmTransactionKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EvmTransactionKind::Legacy => formatter.write_str("legacy"),
            EvmTransactionKind::Eip2930 => formatter.write_str("eip2930"),
            EvmTransactionKind::Eip1559 => formatter.write_str("eip1559"),
            EvmTransactionKind::Eip7702 => formatter.write_str("eip7702"),
        }
    }
}

impl ToBytes for EvmTransactionKind {
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

impl FromBytes for EvmTransactionKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let kind = match tag {
            0 => EvmTransactionKind::Legacy,
            1 => EvmTransactionKind::Eip2930,
            2 => EvmTransactionKind::Eip1559,
            EIP7702_TRANSACTION_TYPE_ID => EvmTransactionKind::Eip7702,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((kind, remainder))
    }
}

/// A Casper approval plus the Ethereum recovery parity for the same signature.
///
/// Casper [`Signature::Secp256k1`] stores the canonical 64-byte ECDSA
/// signature, `r || s`. Ethereum signed transactions carry one extra bit,
/// historically encoded as `v` and in typed transactions as `yParity`, so the
/// sender can be recovered from the transaction payload. `EvmApproval` keeps
/// that Ethereum recovery parity next to the normal Casper approval, allowing
/// the signed Ethereum envelope and transaction hash to be reconstructed
/// without guessing which recovery ID was present in the original payload.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EvmApproval {
    approval: Approval,
    y_parity: bool,
}

impl EvmApproval {
    /// Creates a new EVM approval from a Casper approval and Ethereum recovery parity.
    pub fn new(approval: Approval, y_parity: bool) -> Self {
        EvmApproval { approval, y_parity }
    }

    /// Returns the Casper approval carrying the signer public key and `(r, s)` signature.
    pub fn approval(&self) -> &Approval {
        &self.approval
    }

    /// Returns the Ethereum signature recovery parity.
    pub fn y_parity(&self) -> bool {
        self.y_parity
    }

    /// Returns the public key of the EVM approval's signer.
    pub fn signer(&self) -> &PublicKey {
        self.approval.signer()
    }

    /// Returns the secp256k1 signature stored in the wrapped Casper approval.
    pub fn signature(&self) -> &Signature {
        self.approval.signature()
    }
}

impl ToBytes for EvmApproval {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.approval.serialized_length() + self.y_parity.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.approval.write_bytes(writer)?;
        self.y_parity.write_bytes(writer)
    }
}

impl FromBytes for EvmApproval {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (approval, remainder) = Approval::from_bytes(bytes)?;
        let (y_parity, remainder) = bool::from_bytes(remainder)?;
        Ok((EvmApproval { approval, y_parity }, remainder))
    }
}

/// A signed EIP-7702 authorization-list item.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct SetCodeAuthorization {
    /// Chain ID that scopes the authorization; zero follows EIP-7702 wildcard semantics.
    pub chain_id: U256,
    /// Address whose code the authorized account delegates to.
    pub address: Address,
    /// Nonce expected on the authorizing account.
    pub nonce: u64,
    /// secp256k1 signature recovery parity.
    pub y_parity: u8,
    /// secp256k1 signature `r` value.
    pub r: U256,
    /// secp256k1 signature `s` value.
    pub s: U256,
}

impl SetCodeAuthorization {
    fn from_alloy(value: &AlloyAuthorizationListItem) -> Self {
        SetCodeAuthorization {
            chain_id: alloy_u256_to_casper(*value.chain_id()),
            address: alloy_address_to_address(*value.address()),
            nonce: value.nonce(),
            y_parity: value.y_parity(),
            r: alloy_u256_to_casper(value.r()),
            s: alloy_u256_to_casper(value.s()),
        }
    }

    fn to_alloy(&self) -> AlloyAuthorizationListItem {
        AlloyAuthorizationListItem::new_unchecked(
            AlloyAuthorization {
                chain_id: casper_u256_to_alloy(self.chain_id),
                address: to_alloy_address(self.address),
                nonce: self.nonce,
            },
            self.y_parity,
            casper_u256_to_alloy(self.r),
            casper_u256_to_alloy(self.s),
        )
    }
}

impl ToBytes for SetCodeAuthorization {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.chain_id.serialized_length()
            + self.address.serialized_length()
            + self.nonce.serialized_length()
            + self.y_parity.serialized_length()
            + self.r.serialized_length()
            + self.s.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.chain_id.write_bytes(writer)?;
        self.address.write_bytes(writer)?;
        self.nonce.write_bytes(writer)?;
        self.y_parity.write_bytes(writer)?;
        self.r.write_bytes(writer)?;
        self.s.write_bytes(writer)
    }
}

impl FromBytes for SetCodeAuthorization {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (chain_id, remainder) = U256::from_bytes(bytes)?;
        let (address, remainder) = Address::from_bytes(remainder)?;
        let (nonce, remainder) = u64::from_bytes(remainder)?;
        let (y_parity, remainder) = u8::from_bytes(remainder)?;
        let (r, remainder) = U256::from_bytes(remainder)?;
        let (s, remainder) = U256::from_bytes(remainder)?;
        Ok((
            SetCodeAuthorization {
                chain_id,
                address,
                nonce,
                y_parity,
                r,
                s,
            },
            remainder,
        ))
    }
}

/// Errors returned while decoding or validating EVM transactions.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EvmTransactionError {
    /// The RLP was malformed or not a supported Ethereum envelope.
    Decode(String),
    /// EVM transactions are disabled in the active chainspec.
    Disabled,
    /// The transaction envelope type is not supported by this first-pass executor.
    UnsupportedTransactionType(u8),
    /// The transaction contains an access list, which this first-pass executor does not model.
    UnsupportedAccessList,
    /// Only EIP-7702 transactions may carry a set-code authorization list.
    UnexpectedAuthorizationList,
    /// An EIP-7702 transaction must contain at least one authorization.
    EmptyAuthorizationList,
    /// An EIP-7702 transaction must call an existing target and cannot create a contract.
    MissingSetCodeTarget,
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
        /// EvmTransaction gas price.
        gas_price: u128,
        /// Active block base fee.
        base_fee: u128,
    },
    /// The EIP-1559 maximum fee per gas is lower than the active block base fee.
    MaxFeePerGasBelowBaseFee {
        /// EvmTransaction maximum fee per gas.
        max_fee_per_gas: u128,
        /// Active block base fee.
        base_fee: u128,
    },
    /// The EIP-1559 maximum priority fee per gas must be zero because Casper
    /// does not prioritize transactions based on transaction gas parameters.
    NonZeroMaxPriorityFeePerGas {
        /// EvmTransaction maximum priority fee per gas.
        max_priority_fee_per_gas: u128,
    },
    /// The transaction gas limit exceeds the configured EVM block gas limit.
    GasLimitExceedsBlockGasLimit {
        /// EvmTransaction gas limit.
        gas_limit: u64,
        /// Configured EVM block gas limit.
        block_gas_limit: u64,
    },
    /// The transaction nonce does not match the account nonce in global state.
    InvalidNonce {
        /// Expected account nonce.
        expected: u64,
        /// EvmTransaction nonce.
        actual: u64,
    },
    /// The transaction does not contain an EVM approval.
    MissingApproval,
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

impl Display for EvmTransactionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EvmTransactionError::Decode(error) => {
                write!(formatter, "EVM transaction decode error: {error}")
            }
            EvmTransactionError::Disabled => formatter.write_str("EVM transactions are disabled"),
            EvmTransactionError::UnsupportedTransactionType(kind) => {
                write!(formatter, "unsupported EVM transaction type: {kind}")
            }
            EvmTransactionError::UnsupportedAccessList => {
                formatter.write_str("unsupported EVM transaction access list")
            }
            EvmTransactionError::UnexpectedAuthorizationList => {
                formatter.write_str("unexpected EVM set-code authorization list")
            }
            EvmTransactionError::EmptyAuthorizationList => {
                formatter.write_str("missing EVM set-code authorization list")
            }
            EvmTransactionError::MissingSetCodeTarget => {
                formatter.write_str("missing EVM set-code transaction target")
            }
            EvmTransactionError::MissingChainId => formatter.write_str("missing EVM chain ID"),
            EvmTransactionError::MissingGasPrice => formatter.write_str("missing EVM gas price"),
            EvmTransactionError::MissingTransactionLane => {
                formatter.write_str("missing EVM transaction lane")
            }
            EvmTransactionError::ChainIdMismatch { expected, actual } => {
                write!(
                    formatter,
                    "EVM chain ID mismatch: expected {expected}, got {actual}"
                )
            }
            EvmTransactionError::GasPriceBelowBaseFee {
                gas_price,
                base_fee,
            } => {
                write!(
                    formatter,
                    "EVM gas price {gas_price} is below base fee {base_fee}"
                )
            }
            EvmTransactionError::MaxFeePerGasBelowBaseFee {
                max_fee_per_gas,
                base_fee,
            } => {
                write!(
                    formatter,
                    "EVM max fee per gas {max_fee_per_gas} is below base fee {base_fee}"
                )
            }
            EvmTransactionError::NonZeroMaxPriorityFeePerGas {
                max_priority_fee_per_gas,
            } => {
                write!(
                    formatter,
                    "EVM max priority fee per gas {max_priority_fee_per_gas} must be zero"
                )
            }
            EvmTransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit,
                block_gas_limit,
            } => {
                write!(
                    formatter,
                    "EVM gas limit {gas_limit} exceeds block gas limit {block_gas_limit}"
                )
            }
            EvmTransactionError::InvalidNonce { expected, actual } => {
                write!(
                    formatter,
                    "EVM transaction nonce {actual} does not match account nonce {expected}"
                )
            }
            EvmTransactionError::MissingApproval => formatter.write_str("missing EVM approval"),
            EvmTransactionError::NonSecp256k1Approval => {
                formatter.write_str("EVM approval must use secp256k1")
            }
            EvmTransactionError::InvalidApprovalSignature => {
                formatter.write_str("invalid EVM approval signature")
            }
            EvmTransactionError::SenderMismatch => {
                formatter.write_str("EVM approval signer does not match transaction sender")
            }
            EvmTransactionError::HashMismatch => {
                formatter.write_str("EVM transaction hash does not match approval")
            }
            EvmTransactionError::SenderRecovery(error) => {
                write!(formatter, "EVM transaction sender recovery error: {error}")
            }
            EvmTransactionError::InconsistentEnvelope => {
                formatter.write_str("EVM transaction fields do not match signed envelope")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for EvmTransactionError {}

/// An unsigned Ethereum transaction payload plus one Ethereum-style approval.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EvmTransaction {
    timestamp: Timestamp,
    ttl: TimeDiff,
    initiator_addr: InitiatorAddr,
    hash: EvmTransactionHash,
    from: Address,
    kind: EvmTransactionKind,
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
    authorization_list: Vec<SetCodeAuthorization>,
    approval: Option<EvmApproval>,
}

#[cfg(any(feature = "std", test))]
#[derive(Serialize)]
struct EvmTransactionSerHelper<'a> {
    timestamp: Timestamp,
    ttl: TimeDiff,
    initiator_addr: &'a InitiatorAddr,
    hash: EvmTransactionHash,
    from: Address,
    kind: EvmTransactionKind,
    to: Option<Address>,
    nonce: u64,
    gas_limit: u64,
    gas_price: Option<u128>,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: Option<u128>,
    value: U256,
    input: &'a Vec<u8>,
    chain_id: Option<u64>,
    authorization_list: &'a Vec<SetCodeAuthorization>,
    approval: &'a Option<EvmApproval>,
}

#[cfg(any(feature = "std", test))]
#[derive(Deserialize)]
struct EvmTransactionDeserHelper {
    timestamp: Timestamp,
    ttl: TimeDiff,
    initiator_addr: InitiatorAddr,
    hash: EvmTransactionHash,
    from: Address,
    kind: EvmTransactionKind,
    to: Option<Address>,
    nonce: u64,
    gas_limit: u64,
    gas_price: Option<u128>,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: Option<u128>,
    value: U256,
    input: Vec<u8>,
    chain_id: Option<u64>,
    authorization_list: Vec<SetCodeAuthorization>,
    approval: Option<EvmApproval>,
}

#[cfg(any(feature = "std", test))]
impl Serialize for EvmTransaction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EvmTransactionSerHelper {
            timestamp: self.timestamp,
            ttl: self.ttl,
            initiator_addr: &self.initiator_addr,
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
            authorization_list: &self.authorization_list,
            approval: &self.approval,
        }
        .serialize(serializer)
    }
}

#[cfg(any(feature = "std", test))]
impl<'de> Deserialize<'de> for EvmTransaction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let helper = EvmTransactionDeserHelper::deserialize(deserializer)?;
        let transaction = EvmTransaction {
            timestamp: helper.timestamp,
            ttl: helper.ttl,
            initiator_addr: helper.initiator_addr,
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
            authorization_list: helper.authorization_list,
            approval: helper.approval,
        };
        transaction.verify().map_err(de::Error::custom)?;
        Ok(transaction)
    }
}

impl EvmTransaction {
    /// Constructs an unsigned EVM call transaction for speculative execution.
    ///
    /// This is intended for read-only `eth_call` style execution through the
    /// node's speculative execution path, so it intentionally carries no
    /// approvals and should not be accepted as a network transaction.
    /// The chain ID and gas price are still part of the marker payload so the
    /// node can enforce EVM configuration compliance before execution.
    #[allow(clippy::too_many_arguments)]
    pub fn new_unsigned_call(
        timestamp: Timestamp,
        ttl: TimeDiff,
        initiator_addr: InitiatorAddr,
        chain_id: u64,
        from: Address,
        to: Option<Address>,
        value: U256,
        input: Vec<u8>,
        gas_limit: u64,
        gas_price: u128,
    ) -> Self {
        let mut transaction = EvmTransaction {
            timestamp,
            ttl,
            initiator_addr,
            hash: EvmTransactionHash::default(),
            from,
            kind: EvmTransactionKind::Legacy,
            to,
            nonce: 0,
            gas_limit,
            gas_price: Some(gas_price),
            max_fee_per_gas: 0,
            max_priority_fee_per_gas: None,
            value,
            input,
            chain_id: Some(chain_id),
            authorization_list: Vec::new(),
            approval: None,
        };
        transaction.hash = transaction.unsigned_call_hash();
        transaction
    }

    fn unsigned_call_hash(&self) -> EvmTransactionHash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"casper-evm-call");
        self.timestamp
            .write_bytes(&mut bytes)
            .expect("timestamp should serialize");
        self.ttl
            .write_bytes(&mut bytes)
            .expect("ttl should serialize");
        self.initiator_addr
            .write_bytes(&mut bytes)
            .expect("initiator address should serialize");
        self.from
            .write_bytes(&mut bytes)
            .expect("from address should serialize");
        self.to
            .write_bytes(&mut bytes)
            .expect("to address should serialize");
        self.value
            .write_bytes(&mut bytes)
            .expect("value should serialize");
        Bytes::from(self.input.clone())
            .write_bytes(&mut bytes)
            .expect("input should serialize");
        self.gas_limit
            .write_bytes(&mut bytes)
            .expect("gas limit should serialize");
        self.gas_price
            .write_bytes(&mut bytes)
            .expect("gas price should serialize");
        self.chain_id
            .write_bytes(&mut bytes)
            .expect("chain ID should serialize");
        EvmTransactionHash::new(Digest::hash(bytes))
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        let input_length = Bytes::from(self.input.clone()).serialized_length();
        let mut field_lengths = vec![
            self.timestamp.serialized_length(),
            self.ttl.serialized_length(),
            self.kind.serialized_length(),
            self.hash.serialized_length(),
            self.from.serialized_length(),
            self.to.serialized_length(),
            self.nonce.serialized_length(),
            self.gas_limit.serialized_length(),
        ];
        match self.kind {
            EvmTransactionKind::Legacy | EvmTransactionKind::Eip2930 => {
                field_lengths.extend([
                    self.gas_price.serialized_length(),
                    self.value.serialized_length(),
                    input_length,
                    self.chain_id.serialized_length(),
                    self.approval.serialized_length(),
                    self.initiator_addr.serialized_length(),
                ]);
            }
            EvmTransactionKind::Eip1559 => {
                field_lengths.extend([
                    self.max_fee_per_gas.serialized_length(),
                    self.max_priority_fee_per_gas.serialized_length(),
                    self.value.serialized_length(),
                    input_length,
                    self.chain_id.serialized_length(),
                    self.approval.serialized_length(),
                    self.initiator_addr.serialized_length(),
                ]);
            }
            EvmTransactionKind::Eip7702 => {
                field_lengths.extend([
                    self.max_fee_per_gas.serialized_length(),
                    self.max_priority_fee_per_gas.serialized_length(),
                    self.value.serialized_length(),
                    input_length,
                    self.chain_id.serialized_length(),
                    self.authorization_list.serialized_length(),
                    self.approval.serialized_length(),
                    self.initiator_addr.serialized_length(),
                ]);
            }
        }
        field_lengths
    }

    /// Returns `true` if this is an unsigned read-only call transaction.
    pub fn is_unsigned_call(&self) -> bool {
        self.approval.is_none()
            && self.hash == self.unsigned_call_hash()
            && self.kind == EvmTransactionKind::Legacy
            && self.nonce == 0
            && self.gas_price.is_some()
            && self.max_fee_per_gas == 0
            && self.max_priority_fee_per_gas.is_none()
            && self.chain_id.is_some()
            && self.authorization_list.is_empty()
    }

    /// Decodes a signed Ethereum RLP transaction into an unsigned payload plus approval.
    pub fn from_signed_rlp(
        raw_signed_rlp: Vec<u8>,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Result<Self, EvmTransactionError> {
        if matches!(raw_signed_rlp.first(), Some(&EIP4844_TRANSACTION_TYPE_ID)) {
            // EIP-4844 is proto-danksharding/blob transaction support. It
            // adds blob-carrying transactions with fields like
            // `max_fee_per_blob_gas` and `blob_versioned_hashes`, plus blob
            // gas accounting, blob base fee validation, KZG
            // commitments/proofs, and separate network representations for
            // blob sidecars. Our current transaction type and executor block
            // context only model normal EVM call/create execution, not blob
            // sidecars, blob fee markets, or block/header blob accounting.
            return Err(EvmTransactionError::UnsupportedTransactionType(
                raw_signed_rlp[0],
            ));
        }

        let mut encoded = raw_signed_rlp.as_slice();
        let envelope = TxEnvelope::decode_2718(&mut encoded)
            .map_err(|error| EvmTransactionError::Decode(format!("{error:?}")))?;
        if !encoded.is_empty() {
            return Err(EvmTransactionError::Decode(
                "trailing bytes after transaction envelope".to_string(),
            ));
        }
        if envelope
            .access_list()
            .is_some_and(|access_list| !access_list.is_empty())
        {
            return Err(EvmTransactionError::UnsupportedAccessList);
        }

        let kind = if envelope.is_legacy() {
            EvmTransactionKind::Legacy
        } else if envelope.is_eip2930() {
            EvmTransactionKind::Eip2930
        } else if envelope.is_eip1559() {
            EvmTransactionKind::Eip1559
        } else if envelope.is_eip7702() {
            EvmTransactionKind::Eip7702
        } else {
            return Err(EvmTransactionError::UnsupportedTransactionType(
                envelope.tx_type() as u8,
            ));
        };
        let to = match envelope.kind() {
            AlloyTxKind::Call(address) => Some(alloy_address_to_address(address)),
            AlloyTxKind::Create => None,
        };
        let signature_hash = envelope.signature_hash();
        let approval = evm_approval_from_alloy_signature(envelope.signature(), &signature_hash)?;
        let initiator_addr = InitiatorAddr::AccountHash(approval.signer().to_account_hash());

        let from = envelope
            .recover_signer()
            .map_err(|error| EvmTransactionError::SenderRecovery(format!("{error:?}")))?;
        let authorization_list = match envelope.as_eip7702() {
            Some(transaction) => {
                if transaction.tx().authorization_list.is_empty() {
                    // Keep raw decode errors precise before constructing a
                    // transaction that `verify` would reject anyway.
                    return Err(EvmTransactionError::EmptyAuthorizationList);
                }
                transaction
                    .tx()
                    .authorization_list
                    .iter()
                    .map(SetCodeAuthorization::from_alloy)
                    .collect()
            }
            None => Vec::new(),
        };
        Ok(EvmTransaction {
            timestamp,
            ttl,
            initiator_addr,
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
            authorization_list,
            approval: Some(approval),
        })
    }

    /// Reconstructs the signed Ethereum envelope and validates sender/hash consistency.
    pub fn verify(&self) -> Result<(), EvmTransactionError> {
        let signed = self.signed_envelope()?;
        if b256_to_transaction_hash(*signed.tx_hash()) != self.hash {
            return Err(EvmTransactionError::HashMismatch);
        }
        let recovered = signed
            .recover_signer()
            .map_err(|error| EvmTransactionError::SenderRecovery(format!("{error:?}")))?;
        if alloy_address_to_address(recovered) != self.from {
            return Err(EvmTransactionError::SenderMismatch);
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
    pub fn try_sign(&mut self, secret_key: &SecretKey) -> Result<(), EvmTransactionError> {
        let SecretKey::Secp256k1(signing_key) = secret_key else {
            return Err(EvmTransactionError::NonSecp256k1Approval);
        };
        let unsigned = self.unsigned_transaction()?;
        let signature_hash = unsigned.signature_hash();
        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(signature_hash.as_slice())
            .map_err(|_| EvmTransactionError::InvalidApprovalSignature)?;
        let mut signature_bytes = [0u8; Signature::SECP256K1_LENGTH];
        signature_bytes.copy_from_slice(&signature.to_bytes());
        let signature = Signature::secp256k1(signature_bytes)
            .map_err(|_| EvmTransactionError::InvalidApprovalSignature)?;
        let signer = PublicKey::from(secret_key);
        let initiator_addr = InitiatorAddr::AccountHash(signer.to_account_hash());
        let y_parity = recovery_id.is_y_odd();
        let approval = EvmApproval::new(Approval::new(signer, signature), y_parity);

        let recovered_key = recover_verifying_key(&signature_hash, &signature_bytes, y_parity)?;
        let alloy_signature = AlloySignature::from_bytes_and_parity(&signature_bytes, y_parity);
        let signed = unsigned.into_envelope(alloy_signature);

        self.approval = Some(approval);
        self.initiator_addr = initiator_addr;
        self.from = evm_address_from_verifying_key(&recovered_key);
        self.hash = b256_to_transaction_hash(*signed.tx_hash());
        self.verify()
    }

    /// Returns the raw signed Ethereum RLP bytes reconstructed from the approval.
    pub fn signed_rlp(&self) -> Result<Vec<u8>, EvmTransactionError> {
        Ok(self.signed_envelope()?.encoded_2718())
    }

    /// Returns the raw signed Ethereum RLP bytes reconstructed from the approval.
    pub fn raw_signed_rlp(&self) -> Result<Vec<u8>, EvmTransactionError> {
        self.signed_rlp()
    }

    /// Returns the Ethereum signing hash of the unsigned payload.
    pub fn signature_hash(&self) -> Result<Hash, EvmTransactionError> {
        Ok(b256_to_hash(self.unsigned_transaction()?.signature_hash()))
    }

    /// Returns the bytes Ethereum signs for this unsigned payload.
    pub fn signing_payload(&self) -> Result<Vec<u8>, EvmTransactionError> {
        Ok(self.unsigned_transaction()?.encoded_for_signing())
    }

    /// Returns the approval attached to this transaction, if any.
    pub fn approval(&self) -> Option<&Approval> {
        self.approval.as_ref().map(EvmApproval::approval)
    }

    /// Returns the computed approvals hash identifying this EVM transaction's approval.
    pub fn compute_approvals_hash(&self) -> Result<ApprovalsHash, bytesrepr::Error> {
        let approvals = self.approval().cloned().into_iter().collect();
        ApprovalsHash::compute(&approvals)
    }

    /// Returns the single public key that signed this EVM transaction.
    pub fn signer(&self) -> Result<&PublicKey, EvmTransactionError> {
        Ok(self
            .approval
            .as_ref()
            .ok_or(EvmTransactionError::MissingApproval)?
            .signer())
    }

    /// Returns the Casper initiator address attached to this EVM transaction.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// Returns this transaction with a replacement EVM approval.
    ///
    /// The stored Ethereum transaction hash is intentionally left unchanged;
    /// [`EvmTransaction::verify`] rejects a replacement approval that does not
    /// reconstruct the same signed Ethereum transaction.
    pub fn with_evm_approval(mut self, approval: Option<EvmApproval>) -> Self {
        self.approval = approval;
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
    pub fn hash(&self) -> EvmTransactionHash {
        self.hash
    }

    /// Returns the recovered Ethereum sender address.
    pub fn from(&self) -> Address {
        self.from
    }

    /// Returns the transaction envelope kind.
    pub fn kind(&self) -> EvmTransactionKind {
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

    /// Returns the EIP-7702 set-code authorization list.
    pub fn authorization_list(&self) -> &[SetCodeAuthorization] {
        &self.authorization_list
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
            EvmTransactionKind::Legacy | EvmTransactionKind::Eip2930 => {
                self.gas_price.unwrap_or(self.max_fee_per_gas)
            }
            EvmTransactionKind::Eip1559 | EvmTransactionKind::Eip7702 => {
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

    fn signed_envelope(&self) -> Result<TxEnvelope, EvmTransactionError> {
        let unsigned = self.unsigned_transaction()?;
        let signature_hash = unsigned.signature_hash();
        let (signature, recovered_from) = self.approval_signature(&signature_hash)?;
        if recovered_from != self.from {
            return Err(EvmTransactionError::SenderMismatch);
        }
        Ok(unsigned.into_envelope(signature))
    }

    fn validate_authorization_list(&self) -> Result<(), EvmTransactionError> {
        match self.kind {
            EvmTransactionKind::Eip7702 => {
                if self.authorization_list.is_empty() {
                    return Err(EvmTransactionError::EmptyAuthorizationList);
                }
                if self.to.is_none() {
                    return Err(EvmTransactionError::MissingSetCodeTarget);
                }
            }
            EvmTransactionKind::Legacy
            | EvmTransactionKind::Eip2930
            | EvmTransactionKind::Eip1559 => {
                if !self.authorization_list.is_empty() {
                    return Err(EvmTransactionError::UnexpectedAuthorizationList);
                }
            }
        }
        Ok(())
    }

    fn unsigned_transaction(&self) -> Result<TypedTransaction, EvmTransactionError> {
        self.validate_authorization_list()?;
        let to = match self.to {
            Some(address) => AlloyTxKind::Call(to_alloy_address(address)),
            None => AlloyTxKind::Create,
        };
        let value = casper_u256_to_alloy(self.value);
        let input = AlloyBytes::from(self.input.clone());
        match self.kind {
            EvmTransactionKind::Legacy => Ok(TypedTransaction::Legacy(TxLegacy {
                chain_id: self.chain_id,
                nonce: self.nonce,
                gas_price: self.gas_price.ok_or(EvmTransactionError::MissingGasPrice)?,
                gas_limit: self.gas_limit,
                to,
                value,
                input,
            })),
            EvmTransactionKind::Eip2930 => Ok(TypedTransaction::Eip2930(TxEip2930 {
                chain_id: self.chain_id.ok_or(EvmTransactionError::MissingChainId)?,
                nonce: self.nonce,
                gas_price: self.gas_price.ok_or(EvmTransactionError::MissingGasPrice)?,
                gas_limit: self.gas_limit,
                to,
                value,
                access_list: AccessList::default(),
                input,
            })),
            EvmTransactionKind::Eip1559 => Ok(TypedTransaction::Eip1559(TxEip1559 {
                chain_id: self.chain_id.ok_or(EvmTransactionError::MissingChainId)?,
                nonce: self.nonce,
                gas_limit: self.gas_limit,
                max_fee_per_gas: self.max_fee_per_gas,
                max_priority_fee_per_gas: self.max_priority_fee_per_gas.unwrap_or(0),
                to,
                value,
                access_list: AccessList::default(),
                input,
            })),
            EvmTransactionKind::Eip7702 => {
                let address = self.to.expect("EIP-7702 target validated above");
                Ok(TypedTransaction::Eip7702(TxEip7702 {
                    chain_id: self.chain_id.ok_or(EvmTransactionError::MissingChainId)?,
                    nonce: self.nonce,
                    gas_limit: self.gas_limit,
                    max_fee_per_gas: self.max_fee_per_gas,
                    max_priority_fee_per_gas: self.max_priority_fee_per_gas.unwrap_or(0),
                    to: to_alloy_address(address),
                    value,
                    access_list: AccessList::default(),
                    authorization_list: self
                        .authorization_list
                        .iter()
                        .map(SetCodeAuthorization::to_alloy)
                        .collect(),
                    input,
                }))
            }
        }
    }

    fn approval_signature(
        &self,
        signature_hash: &B256,
    ) -> Result<(AlloySignature, Address), EvmTransactionError> {
        let approval = self
            .approval
            .as_ref()
            .ok_or(EvmTransactionError::MissingApproval)?;
        let raw_signature = secp256k1_signature_bytes(approval)?;
        let expected_signer = approval.signer();
        let y_parity = approval.y_parity();
        let alloy_signature = AlloySignature::from_bytes_and_parity(&raw_signature, y_parity);
        let recovered_key = recover_verifying_key(signature_hash, &raw_signature, y_parity)?;
        let recovered_public_key = public_key_from_verifying_key(&recovered_key)?;
        if &recovered_public_key != expected_signer {
            return Err(EvmTransactionError::InvalidApprovalSignature);
        }
        let expected_initiator_addr = InitiatorAddr::AccountHash(expected_signer.to_account_hash());
        if self.initiator_addr != expected_initiator_addr {
            return Err(EvmTransactionError::InvalidApprovalSignature);
        }
        Ok((
            alloy_signature,
            evm_address_from_verifying_key(&recovered_key),
        ))
    }
}

impl Display for EvmTransaction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EVM transaction {} from {}",
            self.hash.to_hex_string(),
            self.from
        )
    }
}

impl ToBytes for EvmTransaction {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let builder = CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
            .add_field(TIMESTAMP_FIELD_INDEX, &self.timestamp)?
            .add_field(TTL_FIELD_INDEX, &self.ttl)?
            .add_field(KIND_FIELD_INDEX, &self.kind)?
            .add_field(HASH_FIELD_INDEX, &self.hash)?
            .add_field(FROM_FIELD_INDEX, &self.from)?
            .add_field(TO_FIELD_INDEX, &self.to)?
            .add_field(NONCE_FIELD_INDEX, &self.nonce)?
            .add_field(GAS_LIMIT_FIELD_INDEX, &self.gas_limit)?;

        match self.kind {
            EvmTransactionKind::Legacy | EvmTransactionKind::Eip2930 => {
                let input = Bytes::from(self.input.clone());
                builder
                    .add_field(LEGACY_GAS_PRICE_FIELD_INDEX, &self.gas_price)?
                    .add_field(LEGACY_VALUE_FIELD_INDEX, &self.value)?
                    .add_field(LEGACY_INPUT_FIELD_INDEX, &input)?
                    .add_field(LEGACY_CHAIN_ID_FIELD_INDEX, &self.chain_id)?
                    .add_field(LEGACY_APPROVAL_FIELD_INDEX, &self.approval)?
                    .add_field(INITIATOR_ADDR_FIELD_INDEX, &self.initiator_addr)?
                    .binary_payload_bytes()
            }
            EvmTransactionKind::Eip1559 => {
                let input = Bytes::from(self.input.clone());
                builder
                    .add_field(DYNAMIC_MAX_FEE_PER_GAS_FIELD_INDEX, &self.max_fee_per_gas)?
                    .add_field(
                        DYNAMIC_MAX_PRIORITY_FEE_PER_GAS_FIELD_INDEX,
                        &self.max_priority_fee_per_gas,
                    )?
                    .add_field(DYNAMIC_VALUE_FIELD_INDEX, &self.value)?
                    .add_field(DYNAMIC_INPUT_FIELD_INDEX, &input)?
                    .add_field(DYNAMIC_CHAIN_ID_FIELD_INDEX, &self.chain_id)?
                    .add_field(DYNAMIC_APPROVAL_FIELD_INDEX, &self.approval)?
                    .add_field(INITIATOR_ADDR_FIELD_INDEX, &self.initiator_addr)?
                    .binary_payload_bytes()
            }
            EvmTransactionKind::Eip7702 => {
                let input = Bytes::from(self.input.clone());
                builder
                    .add_field(DYNAMIC_MAX_FEE_PER_GAS_FIELD_INDEX, &self.max_fee_per_gas)?
                    .add_field(
                        DYNAMIC_MAX_PRIORITY_FEE_PER_GAS_FIELD_INDEX,
                        &self.max_priority_fee_per_gas,
                    )?
                    .add_field(DYNAMIC_VALUE_FIELD_INDEX, &self.value)?
                    .add_field(DYNAMIC_INPUT_FIELD_INDEX, &input)?
                    .add_field(DYNAMIC_CHAIN_ID_FIELD_INDEX, &self.chain_id)?
                    .add_field(
                        EIP7702_AUTHORIZATION_LIST_FIELD_INDEX,
                        &self.authorization_list,
                    )?
                    .add_field(EIP7702_APPROVAL_FIELD_INDEX, &self.approval)?
                    .add_field(INITIATOR_ADDR_FIELD_INDEX, &self.initiator_addr)?
                    .binary_payload_bytes()
            }
        }
    }

    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for EvmTransaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Self::from_calltable_bytes(bytes)
    }
}

impl EvmTransaction {
    fn from_calltable_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (binary_payload, remainder) =
            CalltableSerializationEnvelope::from_bytes(EVM_TRANSACTION_MAX_CURRENT_FIELDS, bytes)?;
        let window = binary_payload
            .start_consuming()?
            .ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(TIMESTAMP_FIELD_INDEX)?;
        let (timestamp, window) = window.deserialize_and_maybe_next::<Timestamp>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(TTL_FIELD_INDEX)?;
        let (ttl, window) = window.deserialize_and_maybe_next::<TimeDiff>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(KIND_FIELD_INDEX)?;
        let (kind, window) = window.deserialize_and_maybe_next::<EvmTransactionKind>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(HASH_FIELD_INDEX)?;
        let (hash, window) = window.deserialize_and_maybe_next::<EvmTransactionHash>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(FROM_FIELD_INDEX)?;
        let (from, window) = window.deserialize_and_maybe_next::<Address>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(TO_FIELD_INDEX)?;
        let (to, window) = window.deserialize_and_maybe_next::<Option<Address>>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(NONCE_FIELD_INDEX)?;
        let (nonce, window) = window.deserialize_and_maybe_next::<u64>()?;
        let window = window.ok_or(bytesrepr::Error::Formatting)?;
        window.verify_index(GAS_LIMIT_FIELD_INDEX)?;
        let (gas_limit, window) = window.deserialize_and_maybe_next::<u64>()?;

        let transaction = match kind {
            EvmTransactionKind::Legacy | EvmTransactionKind::Eip2930 => {
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(LEGACY_GAS_PRICE_FIELD_INDEX)?;
                let (gas_price, window) = window.deserialize_and_maybe_next::<Option<u128>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(LEGACY_VALUE_FIELD_INDEX)?;
                let (value, window) = window.deserialize_and_maybe_next::<U256>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(LEGACY_INPUT_FIELD_INDEX)?;
                let (input, window) = window.deserialize_and_maybe_next::<Bytes>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(LEGACY_CHAIN_ID_FIELD_INDEX)?;
                let (chain_id, window) = window.deserialize_and_maybe_next::<Option<u64>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(LEGACY_APPROVAL_FIELD_INDEX)?;
                let (approval, window) =
                    window.deserialize_and_maybe_next::<Option<EvmApproval>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(INITIATOR_ADDR_FIELD_INDEX)?;
                let (initiator_addr, window) =
                    window.deserialize_and_maybe_next::<InitiatorAddr>()?;
                if window.is_some() {
                    return Err(bytesrepr::Error::Formatting);
                }
                let max_fee_per_gas = if approval.is_none() {
                    0
                } else {
                    gas_price.unwrap_or_default()
                };
                EvmTransaction {
                    timestamp,
                    ttl,
                    initiator_addr,
                    hash,
                    from,
                    kind,
                    to,
                    nonce,
                    gas_limit,
                    gas_price,
                    max_fee_per_gas,
                    max_priority_fee_per_gas: None,
                    value,
                    input: input.into(),
                    chain_id,
                    authorization_list: Vec::new(),
                    approval,
                }
            }
            EvmTransactionKind::Eip1559 => {
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_MAX_FEE_PER_GAS_FIELD_INDEX)?;
                let (max_fee_per_gas, window) = window.deserialize_and_maybe_next::<u128>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_MAX_PRIORITY_FEE_PER_GAS_FIELD_INDEX)?;
                let (max_priority_fee_per_gas, window) =
                    window.deserialize_and_maybe_next::<Option<u128>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_VALUE_FIELD_INDEX)?;
                let (value, window) = window.deserialize_and_maybe_next::<U256>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_INPUT_FIELD_INDEX)?;
                let (input, window) = window.deserialize_and_maybe_next::<Bytes>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_CHAIN_ID_FIELD_INDEX)?;
                let (chain_id, window) = window.deserialize_and_maybe_next::<Option<u64>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_APPROVAL_FIELD_INDEX)?;
                let (approval, window) =
                    window.deserialize_and_maybe_next::<Option<EvmApproval>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(INITIATOR_ADDR_FIELD_INDEX)?;
                let (initiator_addr, window) =
                    window.deserialize_and_maybe_next::<InitiatorAddr>()?;
                if window.is_some() {
                    return Err(bytesrepr::Error::Formatting);
                }
                EvmTransaction {
                    timestamp,
                    ttl,
                    initiator_addr,
                    hash,
                    from,
                    kind,
                    to,
                    nonce,
                    gas_limit,
                    gas_price: None,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    value,
                    input: input.into(),
                    chain_id,
                    authorization_list: Vec::new(),
                    approval,
                }
            }
            EvmTransactionKind::Eip7702 => {
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_MAX_FEE_PER_GAS_FIELD_INDEX)?;
                let (max_fee_per_gas, window) = window.deserialize_and_maybe_next::<u128>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_MAX_PRIORITY_FEE_PER_GAS_FIELD_INDEX)?;
                let (max_priority_fee_per_gas, window) =
                    window.deserialize_and_maybe_next::<Option<u128>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_VALUE_FIELD_INDEX)?;
                let (value, window) = window.deserialize_and_maybe_next::<U256>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_INPUT_FIELD_INDEX)?;
                let (input, window) = window.deserialize_and_maybe_next::<Bytes>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(DYNAMIC_CHAIN_ID_FIELD_INDEX)?;
                let (chain_id, window) = window.deserialize_and_maybe_next::<Option<u64>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(EIP7702_AUTHORIZATION_LIST_FIELD_INDEX)?;
                let (authorization_list, window) =
                    window.deserialize_and_maybe_next::<Vec<SetCodeAuthorization>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(EIP7702_APPROVAL_FIELD_INDEX)?;
                let (approval, window) =
                    window.deserialize_and_maybe_next::<Option<EvmApproval>>()?;
                let window = window.ok_or(bytesrepr::Error::Formatting)?;
                window.verify_index(INITIATOR_ADDR_FIELD_INDEX)?;
                let (initiator_addr, window) =
                    window.deserialize_and_maybe_next::<InitiatorAddr>()?;
                if window.is_some() {
                    return Err(bytesrepr::Error::Formatting);
                }
                EvmTransaction {
                    timestamp,
                    ttl,
                    initiator_addr,
                    hash,
                    from,
                    kind,
                    to,
                    nonce,
                    gas_limit,
                    gas_price: None,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    value,
                    input: input.into(),
                    chain_id,
                    authorization_list,
                    approval,
                }
            }
        };
        Self::finish_from_bytes(transaction, remainder)
    }

    fn finish_from_bytes(
        transaction: EvmTransaction,
        remainder: &[u8],
    ) -> Result<(Self, &[u8]), bytesrepr::Error> {
        if !transaction.is_unsigned_call() {
            transaction
                .verify()
                .map_err(|_| bytesrepr::Error::Formatting)?;
        }
        Ok((transaction, remainder))
    }
}

fn evm_approval_from_alloy_signature(
    signature: &AlloySignature,
    signature_hash: &B256,
) -> Result<EvmApproval, EvmTransactionError> {
    let y_parity = signature.v();
    let raw_signature = signature.as_rsy();
    let mut signature_bytes = [0u8; Signature::SECP256K1_LENGTH];
    signature_bytes.copy_from_slice(&raw_signature[..Signature::SECP256K1_LENGTH]);
    let recovered_key = recover_verifying_key(signature_hash, &signature_bytes, y_parity)?;
    let signer = public_key_from_verifying_key(&recovered_key)?;
    let signature = Signature::secp256k1(signature_bytes)
        .map_err(|_| EvmTransactionError::InvalidApprovalSignature)?;
    let approval = Approval::new(signer, signature);
    Ok(EvmApproval::new(approval, y_parity))
}

fn secp256k1_signature_bytes(approval: &EvmApproval) -> Result<[u8; 64], EvmTransactionError> {
    if !matches!(approval.signer(), PublicKey::Secp256k1(_))
        || !matches!(approval.signature(), Signature::Secp256k1(_))
    {
        return Err(EvmTransactionError::NonSecp256k1Approval);
    }
    let signature_bytes = Vec::<u8>::from(approval.signature());
    signature_bytes
        .try_into()
        .map_err(|_| EvmTransactionError::InvalidApprovalSignature)
}

fn recover_verifying_key(
    signature_hash: &B256,
    signature_bytes: &[u8; 64],
    y_parity: bool,
) -> Result<VerifyingKey, EvmTransactionError> {
    let signature = K256Signature::try_from(signature_bytes.as_slice())
        .map_err(|_| EvmTransactionError::InvalidApprovalSignature)?;
    let recovery_id = RecoveryId::new(y_parity, false);
    VerifyingKey::recover_from_prehash(signature_hash.as_slice(), &signature, recovery_id)
        .map_err(|_| EvmTransactionError::InvalidApprovalSignature)
}

fn public_key_from_verifying_key(key: &VerifyingKey) -> Result<PublicKey, EvmTransactionError> {
    PublicKey::secp256k1_from_bytes(key.to_encoded_point(true).as_ref())
        .map_err(|_| EvmTransactionError::InvalidApprovalSignature)
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

fn b256_to_transaction_hash(hash: B256) -> EvmTransactionHash {
    EvmTransactionHash::from_raw(hash.0)
}

fn alloy_u256_to_casper(value: AlloyU256) -> U256 {
    U256::from_big_endian(&value.to_be_bytes::<32>())
}

fn casper_u256_to_alloy(value: U256) -> AlloyU256 {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    AlloyU256::from_be_slice(&bytes)
}

#[cfg(test)]
mod tests {
    use alloy_consensus::crypto::secp256k1;

    use super::*;

    const SIGNING_SECRET: [u8; 32] = [7; 32];
    const AUTHORIZATION_SECRET: [u8; 32] = [8; 32];

    #[test]
    fn eip7702_transaction_serde_roundtrips_authorization_list() {
        let transaction = signed_eip7702_transaction();

        let serialized = serde_json::to_string(&transaction).expect("transaction should serialize");
        assert!(serialized.contains("authorization_list"));
        let deserialized: EvmTransaction =
            serde_json::from_str(&serialized).expect("transaction should deserialize");

        assert_eq!(deserialized, transaction);
        assert_eq!(
            deserialized.authorization_list(),
            transaction.authorization_list()
        );
    }

    #[test]
    fn non_eip7702_transaction_serde_rejects_authorization_list() {
        let mut transaction = signed_legacy_transaction();
        transaction
            .authorization_list
            .push(set_code_authorization());
        let serialized = serde_json::to_string(&transaction).expect("transaction should serialize");

        let error = serde_json::from_str::<EvmTransaction>(&serialized)
            .expect_err("transaction should fail");

        assert!(error
            .to_string()
            .contains("unexpected EVM set-code authorization list"));
    }

    #[test]
    fn unsigned_call_transaction_bytesrepr_roundtrips_without_approvals() {
        let transaction = EvmTransaction::new_unsigned_call(
            Timestamp::zero(),
            TimeDiff::from_seconds(300),
            test_initiator_addr(),
            7,
            Address::new([1; crate::evm::ADDRESS_LENGTH]),
            Some(Address::new([2; crate::evm::ADDRESS_LENGTH])),
            U256::from(3),
            vec![0xde, 0xad],
            1_000,
            1,
        );

        assert!(transaction.approval().is_none());
        assert!(transaction.is_unsigned_call());
        assert!(matches!(
            transaction.verify(),
            Err(EvmTransactionError::MissingApproval)
        ));
        bytesrepr::test_serialization_roundtrip(&transaction);
    }

    #[test]
    fn signed_legacy_transaction_bytesrepr_roundtrips() {
        let transaction = signed_legacy_transaction();

        assert_eq!(
            transaction.max_fee_per_gas(),
            transaction
                .gas_price()
                .expect("legacy gas price should exist")
        );
        bytesrepr::test_serialization_roundtrip(&transaction);
    }

    #[test]
    fn non_marker_unsigned_transaction_bytesrepr_is_rejected() {
        let mut transaction = EvmTransaction::new_unsigned_call(
            Timestamp::zero(),
            TimeDiff::from_seconds(300),
            test_initiator_addr(),
            7,
            Address::new([1; crate::evm::ADDRESS_LENGTH]),
            Some(Address::new([2; crate::evm::ADDRESS_LENGTH])),
            U256::from(3),
            vec![0xde, 0xad],
            1_000,
            1,
        );
        transaction.gas_price = Some(2);

        assert!(transaction.approval().is_none());
        assert!(!transaction.is_unsigned_call());
        assert!(EvmTransaction::from_bytes(
            &transaction
                .to_bytes()
                .expect("transaction should serialize")
        )
        .is_err());
    }

    fn signed_legacy_transaction() -> EvmTransaction {
        let tx = TxLegacy {
            chain_id: Some(7),
            nonce: 3,
            gas_price: 1,
            gas_limit: 21_000,
            to: AlloyTxKind::Call(AlloyAddress::from([4; 20])),
            value: AlloyU256::ZERO,
            input: AlloyBytes::default(),
        };
        let transaction_signature =
            secp256k1::sign_message(B256::from(SIGNING_SECRET), tx.signature_hash())
                .expect("transaction signing should succeed");
        let envelope: TxEnvelope = tx.into_signed(transaction_signature).into();

        EvmTransaction::from_signed_rlp(
            envelope.encoded_2718(),
            Timestamp::zero(),
            TimeDiff::from_seconds(60),
        )
        .expect("transaction should decode")
    }

    fn test_initiator_addr() -> InitiatorAddr {
        InitiatorAddr::AccountHash(crate::account::AccountHash::new([9; 32]))
    }

    fn signed_eip7702_transaction() -> EvmTransaction {
        let authorization = AlloyAuthorization {
            chain_id: AlloyU256::from(7),
            address: AlloyAddress::from([9; 20]),
            nonce: 4,
        };
        let authorization_signature = secp256k1::sign_message(
            B256::from(AUTHORIZATION_SECRET),
            authorization.signature_hash(),
        )
        .expect("authorization signing should succeed");
        let tx = TxEip7702 {
            chain_id: 7,
            nonce: 3,
            gas_limit: 70_000,
            max_fee_per_gas: 2_000_000_000,
            max_priority_fee_per_gas: 0,
            to: AlloyAddress::from([4; 20]),
            value: AlloyU256::from(987u64),
            access_list: AccessList::default(),
            authorization_list: vec![authorization.into_signed(authorization_signature)],
            input: AlloyBytes::from(vec![0xde, 0xad]),
        };
        let transaction_signature =
            secp256k1::sign_message(B256::from(SIGNING_SECRET), tx.signature_hash())
                .expect("transaction signing should succeed");
        let envelope: TxEnvelope = tx.into_signed(transaction_signature).into();

        EvmTransaction::from_signed_rlp(
            envelope.encoded_2718(),
            Timestamp::zero(),
            TimeDiff::from_seconds(60),
        )
        .expect("transaction should decode")
    }

    fn set_code_authorization() -> SetCodeAuthorization {
        let authorization = AlloyAuthorization {
            chain_id: AlloyU256::from(7),
            address: AlloyAddress::from([9; 20]),
            nonce: 4,
        };
        let signature = secp256k1::sign_message(
            B256::from(AUTHORIZATION_SECRET),
            authorization.signature_hash(),
        )
        .expect("authorization signing should succeed");
        SetCodeAuthorization::from_alloy(&authorization.into_signed(signature))
    }
}
