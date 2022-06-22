// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    array::TryFromSliceError,
    cmp,
    collections::{BTreeSet, HashMap},
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

use datasize::DataSize;
use derive_more::Display;
use itertools::Itertools;
use num_traits::Zero;
use once_cell::sync::{Lazy, OnceCell};
#[cfg(test)]
use rand::{Rng, RngCore};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

#[cfg(test)]
use casper_execution_engine::core::engine_state::MAX_PAYMENT;
use casper_execution_engine::core::engine_state::{
    executable_deploy_item::ExecutableDeployItem, DeployItem,
};
use casper_hashing::Digest;
#[cfg(test)]
use casper_types::{bytesrepr::Bytes, testing::TestRng};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, runtime_args,
    system::standard_payment::ARG_AMOUNT,
    EraId, ExecutionResult, Motes, PublicKey, RuntimeArgs, SecretKey, Signature, TimeDiff,
    Timestamp, U512,
};

use super::{BlockHash, BlockHashAndHeight, Item, Tag};
use crate::{
    components::block_proposer::DeployInfo,
    rpcs::docs::DocExample,
    types::chainspec::DeployConfig,
    utils::{ds, DisplayIter},
};

static DEPLOY: Lazy<Deploy> = Lazy::new(|| {
    let payment_args = runtime_args! {
        "amount" => 1000
    };
    let payment = ExecutableDeployItem::StoredContractByName {
        name: String::from("casper-example"),
        entry_point: String::from("example-entry-point"),
        args: payment_args,
    };
    let session_args = runtime_args! {
        "amount" => 1000
    };
    let session = ExecutableDeployItem::Transfer { args: session_args };
    let serialized_body = serialize_body(&payment, &session);
    let body_hash = Digest::hash(&serialized_body);

    let secret_key = SecretKey::doc_example();
    let header = DeployHeader {
        account: PublicKey::from(secret_key),
        timestamp: *Timestamp::doc_example(),
        ttl: TimeDiff::from(3_600_000),
        gas_price: 1,
        body_hash,
        dependencies: vec![DeployHash::new(Digest::from([1u8; Digest::LENGTH]))],
        chain_name: String::from("casper-example"),
    };
    let serialized_header = serialize_header(&header);
    let hash = DeployHash::new(Digest::hash(&serialized_header));

    let mut approvals = BTreeSet::new();
    let approval = Approval::create(&hash, secret_key);
    approvals.insert(approval);

    Deploy {
        hash,
        header,
        payment,
        session,
        approvals,
        is_valid: OnceCell::new(),
    }
});

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub enum DeployConfigurationFailure {
    /// Invalid chain name.
    #[error("invalid chain name: expected {expected}, got {got}")]
    InvalidChainName {
        /// The expected chain name.
        expected: String,
        /// The received chain name.
        got: String,
    },

    /// Too many dependencies.
    #[error("{got} dependencies exceeds limit of {max_dependencies}")]
    ExcessiveDependencies {
        /// The dependencies limit.
        max_dependencies: u8,
        /// The actual number of dependencies provided.
        got: usize,
    },

    /// Deploy is too large.
    #[error("deploy size too large: {0}")]
    ExcessiveSize(#[from] ExcessiveSizeError),

    /// Excessive time-to-live.
    #[error("time-to-live of {got} exceeds limit of {max_ttl}")]
    ExcessiveTimeToLive {
        /// The time-to-live limit.
        max_ttl: TimeDiff,
        /// The received time-to-live.
        got: TimeDiff,
    },

    /// The provided body hash does not match the actual hash of the body.
    #[error("the provided body hash does not match the actual hash of the body")]
    InvalidBodyHash,

    /// The provided deploy hash does not match the actual hash of the deploy.
    #[error("the provided hash does not match the actual hash of the deploy")]
    InvalidDeployHash,

    /// The deploy has no approvals.
    #[error("the deploy has no approvals")]
    EmptyApprovals,

    /// Invalid approval.
    #[error("the approval at index {index} is invalid: {error_msg}")]
    InvalidApproval {
        /// The index of the approval at fault.
        index: usize,
        /// The approval validation error.
        error_msg: String,
    },

    /// Excessive length of deploy's session args.
    #[error("serialized session code runtime args of {got} exceeds limit of {max_length}")]
    ExcessiveSessionArgsLength {
        /// The byte size limit of session arguments.
        max_length: usize,
        /// The received length of session arguments.
        got: usize,
    },

    /// Excessive length of deploy's payment args.
    #[error("serialized payment code runtime args of {got} exceeds limit of {max_length}")]
    ExcessivePaymentArgsLength {
        /// The byte size limit of payment arguments.
        max_length: usize,
        /// The received length of payment arguments.
        got: usize,
    },

    /// Missing payment "amount" runtime argument.
    #[error("missing payment 'amount' runtime argument ")]
    MissingPaymentAmount,

    /// Failed to parse payment "amount" runtime argument.
    #[error("failed to parse payment 'amount' as U512")]
    FailedToParsePaymentAmount,

    /// The payment amount associated with the deploy exceeds the block gas limit.
    #[error("payment amount of {got} exceeds the block gas limit of {block_gas_limit}")]
    ExceededBlockGasLimit {
        /// Configured block gas limit.
        block_gas_limit: u64,
        /// The payment amount received.
        got: U512,
    },

    /// Missing payment "amount" runtime argument
    #[error("missing transfer 'amount' runtime argument")]
    MissingTransferAmount,

    /// Failed to parse transfer "amount" runtime argument.
    #[error("failed to parse transfer 'amount' as U512")]
    FailedToParseTransferAmount,

    /// Insufficient transfer amount.
    #[error("insufficient transfer amount; minimum: {minimum} attempted: {attempted}")]
    InsufficientTransferAmount {
        /// The minimum transfer amount.
        minimum: U512,
        /// The attempted transfer amount.
        attempted: U512,
    },

    /// The amount of approvals on the deploy exceeds the max_associated_keys limit.
    #[error("number of associated keys {got} exceeds the maximum {max_associated_keys}")]
    ExcessiveApprovals {
        /// Number of approvals on the deploy.
        got: u32,
        /// The chainspec limit for max_associated_keys.
        max_associated_keys: u32,
    },
}

/// Error returned when a Deploy is too large.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
#[error("deploy size of {actual_deploy_size} bytes exceeds limit of {max_deploy_size}")]
pub struct ExcessiveSizeError {
    /// The maximum permitted serialized deploy size, in bytes.
    pub max_deploy_size: u32,
    /// The serialized size of the deploy provided, in bytes.
    pub actual_deploy_size: usize,
}

/// Errors other than validation failures relating to `Deploy`s.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding to JSON.
    #[error("encoding to JSON: {0}")]
    EncodeToJson(#[from] serde_json::Error),

    /// Error while decoding from JSON.
    #[error("decoding from JSON: {0}")]
    DecodeFromJson(Box<dyn StdError>),

    /// Failed to get "amount" from `payment()`'s runtime args.
    #[error("invalid payment: missing \"amount\" arg")]
    InvalidPayment,
}

impl From<base16::DecodeError> for Error {
    fn from(error: base16::DecodeError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

impl From<TryFromSliceError> for Error {
    fn from(error: TryFromSliceError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

/// The cryptographic hash of a [`Deploy`](struct.Deploy.html).
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    Default,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
#[schemars(with = "String", description = "Hex-encoded deploy hash.")]
pub struct DeployHash(#[schemars(skip)] Digest);

impl DeployHash {
    /// Constructs a new `DeployHash`.
    pub fn new(hash: Digest) -> Self {
        DeployHash(hash)
    }

    /// Returns the wrapped inner hash.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Creates a random deploy hash.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        DeployHash(hash)
    }
}

impl From<DeployHash> for casper_types::DeployHash {
    fn from(deploy_hash: DeployHash) -> casper_types::DeployHash {
        casper_types::DeployHash::new(deploy_hash.inner().value())
    }
}

impl From<casper_types::DeployHash> for DeployHash {
    fn from(deploy_hash: casper_types::DeployHash) -> DeployHash {
        DeployHash::new(deploy_hash.value().into())
    }
}

impl From<DeployHash> for Digest {
    fn from(deploy_hash: DeployHash) -> Self {
        deploy_hash.0
    }
}

impl Display for DeployHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "deploy-hash({})", self.0,)
    }
}

impl From<Digest> for DeployHash {
    fn from(digest: Digest) -> Self {
        Self(digest)
    }
}

impl AsRef<[u8]> for DeployHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for DeployHash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for DeployHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(inner, remainder)| (DeployHash(inner), remainder))
    }
}

/// The [`DeployHash`](struct.DeployHash.html) stored in a way distinguishing between WASM deploys
/// and transfers.
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    Display,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub enum DeployOrTransferHash {
    /// Hash of a deploy.
    #[display(fmt = "deploy {}", _0)]
    Deploy(DeployHash),
    /// Hash of a transfer.
    #[display(fmt = "transfer {}", _0)]
    Transfer(DeployHash),
}

impl DeployOrTransferHash {
    /// Gets the inner `DeployHash`.
    pub fn deploy_hash(&self) -> &DeployHash {
        match self {
            DeployOrTransferHash::Deploy(hash) | DeployOrTransferHash::Transfer(hash) => hash,
        }
    }

    /// Returns `true` if this is a transfer hash.
    pub fn is_transfer(&self) -> bool {
        matches!(self, DeployOrTransferHash::Transfer(_))
    }
}

impl From<DeployOrTransferHash> for DeployHash {
    fn from(dt_hash: DeployOrTransferHash) -> DeployHash {
        match dt_hash {
            DeployOrTransferHash::Deploy(hash) => hash,
            DeployOrTransferHash::Transfer(hash) => hash,
        }
    }
}

/// The header portion of a [`Deploy`](struct.Deploy.html).
#[derive(
    Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct DeployHeader {
    account: PublicKey,
    timestamp: Timestamp,
    ttl: TimeDiff,
    gas_price: u64,
    body_hash: Digest,
    dependencies: Vec<DeployHash>,
    chain_name: String,
}

impl DeployHeader {
    /// The account within which the deploy will be run.
    pub fn account(&self) -> &PublicKey {
        &self.account
    }

    /// When the deploy was created.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// How long the deploy will stay valid.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    /// Has this deploy expired?
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        let lifespan = self.timestamp.saturating_add(self.ttl);
        lifespan < current_instant
    }

    /// Price per gas unit for this deploy.
    pub fn gas_price(&self) -> u64 {
        self.gas_price
    }

    /// Hash of the Wasm code.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// Other deploys that have to be run before this one.
    pub fn dependencies(&self) -> &Vec<DeployHash> {
        &self.dependencies
    }

    /// Which chain the deploy is supposed to be run on.
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// Determine if this deploy header has valid values based on a `DeployConfig` and timestamp.
    pub fn is_valid(&self, deploy_config: &DeployConfig, current_timestamp: Timestamp) -> bool {
        let ttl_valid = self.ttl() <= deploy_config.max_ttl;
        let timestamp_valid = self.timestamp() <= current_timestamp;
        let not_expired = !self.expired(current_timestamp);
        let num_deps_valid = self.dependencies().len() <= deploy_config.max_dependencies as usize;
        ttl_valid && timestamp_valid && not_expired && num_deps_valid
    }

    /// Returns the timestamp of when the deploy expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp.saturating_add(self.ttl)
    }
}

impl ToBytes for DeployHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.account.to_bytes()?);
        buffer.extend(self.timestamp.to_bytes()?);
        buffer.extend(self.ttl.to_bytes()?);
        buffer.extend(self.gas_price.to_bytes()?);
        buffer.extend(self.body_hash.to_bytes()?);
        buffer.extend(self.dependencies.to_bytes()?);
        buffer.extend(self.chain_name.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.account.serialized_length()
            + self.timestamp.serialized_length()
            + self.ttl.serialized_length()
            + self.gas_price.serialized_length()
            + self.body_hash.serialized_length()
            + self.dependencies.serialized_length()
            + self.chain_name.serialized_length()
    }
}

impl FromBytes for DeployHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account, remainder) = PublicKey::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (gas_price, remainder) = u64::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (dependencies, remainder) = Vec::<DeployHash>::from_bytes(remainder)?;
        let (chain_name, remainder) = String::from_bytes(remainder)?;
        let deploy_header = DeployHeader {
            account,
            timestamp,
            ttl,
            gas_price,
            body_hash,
            dependencies,
            chain_name,
        };
        Ok((deploy_header, remainder))
    }
}

impl Display for DeployHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy-header[account: {}, timestamp: {}, ttl: {}, gas_price: {}, body_hash: {}, dependencies: [{}], chain_name: {}]",
            self.account,
            self.timestamp,
            self.ttl,
            self.gas_price,
            self.body_hash,
            DisplayIter::new(self.dependencies.iter()),
            self.chain_name,
        )
    }
}

/// A struct containing a signature and the public key of the signer.
#[derive(
    Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    signer: PublicKey,
    signature: Signature,
}

impl Approval {
    /// Creates an approval for the given deploy hash using the given secret key.
    pub fn create(hash: &DeployHash, secret_key: &SecretKey) -> Self {
        let signer = PublicKey::from(secret_key);
        let signature = crypto::sign(hash, secret_key, &signer);
        Self { signer, signature }
    }

    /// Returns the public key of the approval's signer.
    pub fn signer(&self) -> &PublicKey {
        &self.signer
    }

    /// Returns the approval signature.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

#[cfg(test)]
impl Approval {
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            signer: PublicKey::random(rng),
            signature: Signature::ed25519([0; Signature::ED25519_LENGTH]).unwrap(),
        }
    }
}

impl Display for Approval {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "approval({})", self.signer)
    }
}

impl ToBytes for Approval {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.signer.to_bytes()?);
        buffer.extend(self.signature.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.signer.serialized_length() + self.signature.serialized_length()
    }
}

impl FromBytes for Approval {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (signer, remainder) = PublicKey::from_bytes(bytes)?;
        let (signature, remainder) = Signature::from_bytes(remainder)?;
        let approval = Approval { signer, signature };
        Ok((approval, remainder))
    }
}

/// The hash of a deploy (or transfer) together with signatures approving it for execution.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeployWithApprovals {
    /// The hash of the deploy.
    deploy_hash: DeployHash,
    /// The approvals for the deploy.
    approvals: BTreeSet<Approval>,
}

impl DeployWithApprovals {
    /// Creates a new `DeployWithApprovals`.
    pub fn new(deploy_hash: DeployHash, approvals: BTreeSet<Approval>) -> Self {
        Self {
            deploy_hash,
            approvals,
        }
    }

    /// Returns the deploy hash.
    pub fn deploy_hash(&self) -> &DeployHash {
        &self.deploy_hash
    }

    /// Returns the approvals.
    pub fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }
}

impl From<&Deploy> for DeployWithApprovals {
    fn from(deploy: &Deploy) -> Self {
        DeployWithApprovals {
            deploy_hash: *deploy.id(),
            approvals: deploy.approvals().clone(),
        }
    }
}

/// A set of approvals that has been agreed upon by consensus to approve of a specific deploy.
#[derive(DataSize, Debug, Deserialize, Eq, PartialEq, Serialize, Clone)]
pub struct FinalizedApprovals(BTreeSet<Approval>);

impl FinalizedApprovals {
    /// Creates a new instance of `FinalizedApprovals`.
    pub fn new(approvals: BTreeSet<Approval>) -> Self {
        Self(approvals)
    }

    /// Return the inner set of approvals.
    pub fn into_inner(self) -> BTreeSet<Approval> {
        self.0
    }
}

impl AsRef<BTreeSet<Approval>> for FinalizedApprovals {
    /// Returns the approvals as a reference to the set.
    fn as_ref(&self) -> &BTreeSet<Approval> {
        &self.0
    }
}

/// A set of finalized approvals together with data identifying the deploy.
#[derive(DataSize, Debug, Deserialize, Eq, PartialEq, Serialize, Clone)]
pub struct FinalizedApprovalsWithId {
    id: DeployHash,
    approvals: FinalizedApprovals,
}

impl FinalizedApprovalsWithId {
    /// Creates a new instance of `FinalizedApprovalsWithId`.
    pub fn new(id: DeployHash, approvals: FinalizedApprovals) -> Self {
        Self { id, approvals }
    }

    /// Return the inner set of approvals.
    pub fn into_inner(self) -> BTreeSet<Approval> {
        self.approvals.into_inner()
    }
}

/// Error type containing the error message passed from `crypto::verify`
#[derive(Debug, Error)]
#[error("invalid approval from {signer}: {error}")]
pub struct FinalizedApprovalsVerificationError {
    signer: PublicKey,
    error: String,
}

impl Item for FinalizedApprovalsWithId {
    type Id = DeployHash;
    type ValidationError = FinalizedApprovalsVerificationError;

    const TAG: Tag = Tag::FinalizedApprovals;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(
        &self,
        _verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        for approval in &self.approvals.0 {
            crypto::verify(&self.id, approval.signature(), approval.signer()).map_err(|err| {
                FinalizedApprovalsVerificationError {
                    signer: approval.signer().clone(),
                    error: format!("{}", err),
                }
            })?;
        }
        Ok(())
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        self.id
    }
}

impl Display for FinalizedApprovalsWithId {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "finalized approvals for {}: {}",
            self.id,
            DisplayIter::new(self.approvals.0.iter())
        )
    }
}

/// A deploy; an item containing a smart contract along with the requester's signature(s).
#[derive(Clone, DataSize, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deploy {
    hash: DeployHash,
    header: DeployHeader,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
    approvals: BTreeSet<Approval>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    is_valid: OnceCell<Result<(), DeployConfigurationFailure>>,
}

impl hash::Hash for Deploy {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Destructure to make sure we don't accidentally omit fields.
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
            is_valid: _,
        } = self;
        hash.hash(state);
        header.hash(state);
        payment.hash(state);
        session.hash(state);
        approvals.hash(state);
    }
}

impl PartialEq for Deploy {
    fn eq(&self, other: &Deploy) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
            is_valid: _,
        } = self;
        *hash == other.hash
            && *header == other.header
            && *payment == other.payment
            && *session == other.session
            && *approvals == other.approvals
    }
}

impl Ord for Deploy {
    fn cmp(&self, other: &Deploy) -> cmp::Ordering {
        // Destructure to make sure we don't accidentally omit fields.
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
            is_valid: _,
        } = self;
        hash.cmp(&other.hash)
            .then_with(|| header.cmp(&other.header))
            .then_with(|| payment.cmp(&other.payment))
            .then_with(|| session.cmp(&other.session))
            .then_with(|| approvals.cmp(&other.approvals))
    }
}

impl PartialOrd for Deploy {
    fn partial_cmp(&self, other: &Deploy) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Deploy {
    /// Constructs a new signed `Deploy`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        timestamp: Timestamp,
        ttl: TimeDiff,
        gas_price: u64,
        dependencies: Vec<DeployHash>,
        chain_name: String,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
        secret_key: &SecretKey,
        account: Option<PublicKey>,
    ) -> Deploy {
        let serialized_body = serialize_body(&payment, &session);
        let body_hash = Digest::hash(&serialized_body);

        let account = account.unwrap_or_else(|| PublicKey::from(secret_key));

        // Remove duplicates.
        let dependencies = dependencies.into_iter().unique().collect();
        let header = DeployHeader {
            account,
            timestamp,
            ttl,
            gas_price,
            body_hash,
            dependencies,
            chain_name,
        };
        let serialized_header = serialize_header(&header);
        let hash = DeployHash::new(Digest::hash(&serialized_header));

        let mut deploy = Deploy {
            hash,
            header,
            payment,
            session,
            approvals: BTreeSet::new(),
            is_valid: OnceCell::new(),
        };

        deploy.sign(secret_key);
        deploy
    }

    /// Adds a signature of this deploy's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let approval = Approval::create(&self.hash, secret_key);
        self.approvals.insert(approval);
    }

    /// Returns the `DeployHash` identifying this `Deploy`.
    pub fn id(&self) -> &DeployHash {
        &self.hash
    }

    /// Returns a reference to the `DeployHeader` of this `Deploy`.
    pub fn header(&self) -> &DeployHeader {
        &self.header
    }

    /// Returns the `DeployHeader` of this `Deploy`.
    pub fn take_header(self) -> DeployHeader {
        self.header
    }

    /// Returns the `ExecutableDeployItem` for payment code.
    pub fn payment(&self) -> &ExecutableDeployItem {
        &self.payment
    }

    /// Returns the `ExecutableDeployItem` for session code.
    pub fn session(&self) -> &ExecutableDeployItem {
        &self.session
    }

    /// Returns the `Approval`s for this deploy.
    pub fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }

    /// Replaces the set of approvals attached to this deploy.
    pub fn replace_approvals(&mut self, approvals: BTreeSet<Approval>) {
        self.approvals = approvals;
    }

    /// Returns the hash of this deploy wrapped in `DeployOrTransferHash`.
    pub fn deploy_or_transfer_hash(&self) -> DeployOrTransferHash {
        if self.session.is_transfer() {
            DeployOrTransferHash::Transfer(self.hash)
        } else {
            DeployOrTransferHash::Deploy(self.hash)
        }
    }

    /// Returns the `DeployInfo`.
    pub fn deploy_info(&self) -> Result<DeployInfo, Error> {
        let header = self.header().clone();
        let size = self.serialized_length();
        let payment_amount = if self.session().is_transfer() {
            // TODO: we need a non-zero value constant for wasm-less transfer cost.
            Motes::zero()
        } else {
            let payment_item = self.payment().clone();

            // In the happy path for a payment we expect:
            // - args to exist
            // - contain "amount"
            // - be a valid U512 value.
            let value = payment_item
                .args()
                .get(ARG_AMOUNT)
                .ok_or(Error::InvalidPayment)?;
            let value = value
                .clone()
                .into_t::<U512>()
                .map_err(|_| Error::InvalidPayment)?;
            Motes::new(value)
        };
        Ok(DeployInfo {
            header,
            payment_amount,
            size,
        })
    }

    /// Returns true if the serialized size of the deploy is not greater than `max_deploy_size`.
    pub fn is_valid_size(&self, max_deploy_size: u32) -> Result<(), ExcessiveSizeError> {
        let deploy_size = self.serialized_length();
        if deploy_size > max_deploy_size as usize {
            return Err(ExcessiveSizeError {
                max_deploy_size,
                actual_deploy_size: deploy_size,
            });
        }
        Ok(())
    }

    /// Returns `Ok` if this block's body hashes to the value of `body_hash` in the header, and if
    /// this block's header hashes to the value claimed as the block hash.  Otherwise returns `Err`.
    pub(crate) fn has_valid_hash(&self) -> Result<(), DeployConfigurationFailure> {
        let serialized_body = serialize_body(&self.payment, &self.session);
        let body_hash = Digest::hash(&serialized_body);
        if body_hash != self.header.body_hash {
            warn!(?self, ?body_hash, "invalid deploy body hash");
            return Err(DeployConfigurationFailure::InvalidBodyHash);
        }

        let serialized_header = serialize_header(&self.header);
        let hash = DeployHash::new(Digest::hash(&serialized_header));
        if hash != self.hash {
            warn!(?self, ?hash, "invalid deploy hash");
            return Err(DeployConfigurationFailure::InvalidDeployHash);
        }
        Ok(())
    }

    /// Returns true if and only if:
    ///   * the deploy hash is correct (should be the hash of the header), and
    ///   * the body hash is correct (should be the hash of the body), and
    ///   * approvals are non empty, and
    ///   * all approvals are valid signatures of the deploy hash
    pub fn is_valid(&self) -> Result<(), DeployConfigurationFailure> {
        self.is_valid.get_or_init(|| validate_deploy(self)).clone()
    }

    /// Returns true if and only if:
    ///   * the chain_name is correct,
    ///   * the configured parameters are complied with,
    pub fn is_config_compliant(
        &self,
        chain_name: &str,
        config: &DeployConfig,
        max_associated_keys: u32,
    ) -> Result<(), DeployConfigurationFailure> {
        self.is_valid_size(config.max_deploy_size)?;

        let header = self.header();
        if header.chain_name() != chain_name {
            info!(
                deploy_hash = %self.id(),
                deploy_header = %header,
                chain_name = %header.chain_name(),
                "invalid chain identifier"
            );
            return Err(DeployConfigurationFailure::InvalidChainName {
                expected: chain_name.to_string(),
                got: header.chain_name().to_string(),
            });
        }

        if header.dependencies().len() > config.max_dependencies as usize {
            info!(
                deploy_hash = %self.id(),
                deploy_header = %header,
                max_dependencies = %config.max_dependencies,
                "deploy dependency ceiling exceeded"
            );
            return Err(DeployConfigurationFailure::ExcessiveDependencies {
                max_dependencies: config.max_dependencies,
                got: header.dependencies().len(),
            });
        }

        if header.ttl() > config.max_ttl {
            info!(
                deploy_hash = %self.id(),
                deploy_header = %header,
                max_ttl = %config.max_ttl,
                "deploy ttl excessive"
            );
            return Err(DeployConfigurationFailure::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: header.ttl(),
            });
        }

        if self.approvals.len() > max_associated_keys as usize {
            info!(
                deploy_hash = %self.id(),
                number_of_associated_keys = %self.approvals.len(),
                max_associated_keys = %max_associated_keys,
                "number of associated keys exceeds the maximum limit"
            );
            return Err(DeployConfigurationFailure::ExcessiveApprovals {
                got: self.approvals.len() as u32,
                max_associated_keys,
            });
        }

        // Transfers have a fixed cost and won't blow the block gas limit.
        // Other deploys can, therefore, statically check the payment amount
        // associated with the deploy.
        if !self.session().is_transfer() {
            let value = self
                .payment()
                .args()
                .get(ARG_AMOUNT)
                .ok_or(DeployConfigurationFailure::MissingPaymentAmount)?;
            let payment_amount = value
                .clone()
                .into_t::<U512>()
                .map_err(|_| DeployConfigurationFailure::FailedToParsePaymentAmount)?;
            if payment_amount > U512::from(config.block_gas_limit) {
                info!(
                    amount = %payment_amount,
                    block_gas_limit = %config.block_gas_limit, "payment amount exceeds block gas limit"
                );
                return Err(DeployConfigurationFailure::ExceededBlockGasLimit {
                    block_gas_limit: config.block_gas_limit,
                    got: payment_amount,
                });
            }
        }

        let payment_args_length = self.payment().args().serialized_length();
        if payment_args_length > config.payment_args_max_length as usize {
            info!(
                payment_args_length,
                payment_args_max_length = config.payment_args_max_length,
                "payment args excessive"
            );
            return Err(DeployConfigurationFailure::ExcessivePaymentArgsLength {
                max_length: config.payment_args_max_length as usize,
                got: payment_args_length,
            });
        }

        let session_args_length = self.session().args().serialized_length();
        if session_args_length > config.session_args_max_length as usize {
            info!(
                session_args_length,
                session_args_max_length = config.session_args_max_length,
                "session args excessive"
            );
            return Err(DeployConfigurationFailure::ExcessiveSessionArgsLength {
                max_length: config.session_args_max_length as usize,
                got: session_args_length,
            });
        }

        if self.session().is_transfer() {
            let item = self.session().clone();
            let attempted = item
                .args()
                .get(ARG_AMOUNT)
                .ok_or_else(|| {
                    info!("missing transfer 'amount' runtime argument");
                    DeployConfigurationFailure::MissingTransferAmount
                })?
                .clone()
                .into_t::<U512>()
                .map_err(|_| {
                    info!("failed to parse transfer 'amount' runtime argument as a U512");
                    DeployConfigurationFailure::FailedToParseTransferAmount
                })?;
            let minimum = U512::from(config.native_transfer_minimum_motes);
            if attempted < minimum {
                info!(
                    minimum = %config.native_transfer_minimum_motes,
                    amount = %attempted,
                    "insufficient transfer amount"
                );
                return Err(DeployConfigurationFailure::InsufficientTransferAmount {
                    minimum,
                    attempted,
                });
            }
        }

        Ok(())
    }
}

/// A deploy combined with a potential set of finalized approvals.
///
/// Represents a deploy, along with a potential set of approvals different from those contained in
/// the deploy itself. If such a set of approvals is present, it indicates that the set contained
/// the deploy was not the set  used to validate the execution of the deploy after consensus.
///
/// A typical case where these can differ is if a deploy is sent with an original set of approvals
/// to the local node, while a second set of approvals makes it to the proposing node. The local
/// node has to adhere to the proposer's approvals to obtain the same outcome.
#[derive(DataSize, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeployWithFinalizedApprovals {
    /// The deploy that likely has been included in a block.
    deploy: Deploy,
    /// Approvals used to verify the deploy during block execution.
    finalized_approvals: Option<FinalizedApprovals>,
}

impl DeployWithFinalizedApprovals {
    /// Creates a new deploy with finalized approvals from parts.
    pub fn new(deploy: Deploy, finalized_approvals: Option<FinalizedApprovals>) -> Self {
        Self {
            deploy,
            finalized_approvals,
        }
    }

    /// Creates a naive deploy by potentially substituting the approvals with the finalized
    /// approvals.
    pub fn into_naive(self) -> Deploy {
        let mut deploy = self.deploy;
        if let Some(finalized_approvals) = self.finalized_approvals {
            deploy.approvals = finalized_approvals.0;
        }

        deploy
    }

    /// Extracts the original deploy by discarding the finalized approvals.
    pub fn discard_finalized_approvals(self) -> Deploy {
        self.deploy
    }

    #[cfg(test)]
    pub(crate) fn original_approvals(&self) -> &BTreeSet<Approval> {
        self.deploy.approvals()
    }

    #[cfg(test)]
    pub(crate) fn finalized_approvals(&self) -> Option<&FinalizedApprovals> {
        self.finalized_approvals.as_ref()
    }
}

#[cfg(test)]
impl Deploy {
    /// Generates a completely random instance.
    pub fn random(rng: &mut TestRng) -> Self {
        let timestamp = Timestamp::random(rng);
        let ttl = TimeDiff::from(rng.gen_range(60_000..3_600_000));
        Deploy::random_with_timestamp_and_ttl(rng, timestamp, ttl)
    }

    /// Generates a random instance but using the specified `timestamp` and `ttl`.
    pub fn random_with_timestamp_and_ttl(
        rng: &mut TestRng,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Self {
        let gas_price = rng.gen_range(1..100);

        let dependencies = vec![
            DeployHash::new(Digest::hash(rng.next_u64().to_le_bytes())),
            DeployHash::new(Digest::hash(rng.next_u64().to_le_bytes())),
            DeployHash::new(Digest::hash(rng.next_u64().to_le_bytes())),
        ];
        let chain_name = String::from("casper-example");

        // We need "amount" in order to be able to get correct info via `deploy_info()`.
        let payment_args = runtime_args! {
            "amount" => U512::from(10),
        };
        let payment = ExecutableDeployItem::StoredContractByName {
            name: String::from("casper-example"),
            entry_point: String::from("example-entry-point"),
            args: payment_args,
        };

        let session = rng.gen();

        let secret_key = SecretKey::random(rng);

        Deploy::new(
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
    }

    /// Turns `self` into an invalid deploy by clearing the `chain_name`, invalidating the deploy
    /// hash.
    pub(crate) fn invalidate(&mut self) {
        self.header.chain_name.clear();
    }

    pub(crate) fn random_valid_native_transfer(rng: &mut TestRng) -> Self {
        let deploy = Self::random(rng);
        let transfer_args = runtime_args! {
            "amount" => *MAX_PAYMENT,
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let payment_args = runtime_args! {
            "amount" => U512::from(10),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: payment_args,
        };
        let secret_key = SecretKey::random(rng);
        Deploy::new(
            Timestamp::now(),
            deploy.header.ttl,
            deploy.header.gas_price,
            deploy.header.dependencies,
            deploy.header.chain_name,
            payment,
            session,
            &secret_key,
            None,
        )
    }

    pub(crate) fn random_valid_native_transfer_without_deps(rng: &mut TestRng) -> Self {
        let deploy = Self::random(rng);
        let transfer_args = runtime_args! {
            "amount" => *MAX_PAYMENT,
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let payment_args = runtime_args! {
            "amount" => U512::from(10),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: payment_args,
        };
        let secret_key = SecretKey::random(rng);
        Deploy::new(
            Timestamp::now(),
            deploy.header.ttl,
            deploy.header.gas_price,
            vec![],
            deploy.header.chain_name,
            payment,
            session,
            &secret_key,
            None,
        )
    }

    pub(crate) fn random_without_payment_amount(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: RuntimeArgs::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_mangled_payment_amount(rng: &mut TestRng) -> Self {
        let payment_args = runtime_args! {
            "amount" => "invalid-argument"
        };
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: payment_args,
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_valid_custom_payment_contract_by_name(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByName {
            name: "Test".to_string(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_missing_payment_contract_by_hash(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_missing_entry_point_in_payment_contract(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_valid_custom_payment_package_by_name(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByName {
            name: "Test".to_string(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_missing_payment_package_by_hash(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: Default::default(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_nonexistent_contract_version_in_payment_package(
        rng: &mut TestRng,
    ) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: [19; 32].into(),
            version: Some(6u32),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    pub(crate) fn random_with_valid_session_contract_by_name(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByName {
            name: "Test".to_string(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_missing_session_contract_by_hash(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: Default::default(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_missing_entry_point_in_session_contract(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_valid_session_package_by_name(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByName {
            name: "Test".to_string(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_missing_session_package_by_hash(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: Default::default(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_nonexistent_contract_version_in_session_package(
        rng: &mut TestRng,
    ) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: [19; 32].into(),
            version: Some(6u32),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_without_transfer_target(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "amount" => *MAX_PAYMENT,
            "source" => PublicKey::random(rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_without_transfer_amount(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_mangled_transfer_amount(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "amount" => "mangled-transfer-amount",
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_with_empty_session_module_bytes(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    pub(crate) fn random_expired_deploy(rng: &mut TestRng) -> Self {
        let deploy = Self::random_valid_native_transfer(rng);
        let secret_key = SecretKey::random(rng);

        Deploy::new(
            Timestamp::zero(),
            TimeDiff::from_seconds(1u32),
            deploy.header.gas_price,
            deploy.header.dependencies,
            deploy.header.chain_name,
            deploy.payment,
            deploy.session,
            &secret_key,
            None,
        )
    }

    /// Creates a deploy with native transfer as payment code.
    pub(crate) fn random_with_native_transfer_in_payment_logic(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "amount" => *MAX_PAYMENT,
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let payment = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    fn random_transfer_with_payment(rng: &mut TestRng, payment: ExecutableDeployItem) -> Self {
        let deploy = Self::random_valid_native_transfer(rng);
        let secret_key = SecretKey::random(rng);

        Deploy::new(
            deploy.header.timestamp,
            deploy.header.ttl,
            deploy.header.gas_price,
            deploy.header.dependencies,
            deploy.header.chain_name,
            payment,
            deploy.session,
            &secret_key,
            None,
        )
    }

    fn random_transfer_with_session(rng: &mut TestRng, session: ExecutableDeployItem) -> Self {
        let deploy = Self::random_valid_native_transfer(rng);
        let secret_key = SecretKey::random(rng);

        Deploy::new(
            deploy.header.timestamp,
            deploy.header.ttl,
            deploy.header.gas_price,
            deploy.header.dependencies,
            deploy.header.chain_name,
            deploy.payment,
            session,
            &secret_key,
            None,
        )
    }
}

impl DocExample for Deploy {
    fn doc_example() -> &'static Self {
        &*DEPLOY
    }
}

fn serialize_header(header: &DeployHeader) -> Vec<u8> {
    header
        .to_bytes()
        .unwrap_or_else(|error| panic!("should serialize deploy header: {}", error))
}

fn serialize_body(payment: &ExecutableDeployItem, session: &ExecutableDeployItem) -> Vec<u8> {
    let mut buffer = payment
        .to_bytes()
        .unwrap_or_else(|error| panic!("should serialize payment code: {}", error));
    buffer.extend(
        session
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize session code: {}", error)),
    );
    buffer
}

// Computationally expensive validity check for a given deploy instance, including
// asymmetric_key signing verification.
fn validate_deploy(deploy: &Deploy) -> Result<(), DeployConfigurationFailure> {
    if deploy.approvals.is_empty() {
        warn!(?deploy, "deploy has no approvals");
        return Err(DeployConfigurationFailure::EmptyApprovals);
    }

    deploy.has_valid_hash()?;

    for (index, approval) in deploy.approvals.iter().enumerate() {
        if let Err(error) = crypto::verify(&deploy.hash, &approval.signature, &approval.signer) {
            warn!(?deploy, "failed to verify approval {}: {}", index, error);
            return Err(DeployConfigurationFailure::InvalidApproval {
                index,
                error_msg: error.to_string(),
            });
        }
    }

    Ok(())
}

impl Item for Deploy {
    type Id = DeployHash;
    type ValidationError = DeployConfigurationFailure;

    const TAG: Tag = Tag::Deploy;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(
        &self,
        _verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        // TODO: Validate approvals later, and only if the approvers are actually authorized!
        validate_deploy(self)
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        *self.id()
    }
}

impl Display for Deploy {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy[{}, {}, payment_code: {}, session_code: {}, approvals: {}]",
            self.hash,
            self.header,
            self.payment,
            self.session,
            DisplayIter::new(self.approvals.iter())
        )
    }
}

impl From<Deploy> for DeployItem {
    fn from(deploy: Deploy) -> Self {
        let address = deploy.header().account().to_account_hash();
        let authorization_keys = deploy
            .approvals()
            .iter()
            .map(|approval| approval.signer().to_account_hash())
            .collect();

        DeployItem::new(
            address,
            deploy.session().clone(),
            deploy.payment().clone(),
            deploy.header().gas_price(),
            authorization_keys,
            casper_types::DeployHash::new(deploy.id().inner().value()),
        )
    }
}

/// The deploy mutable metadata.
///
/// Currently a stop-gap measure to associate an immutable deploy with additional metadata. Holds
/// execution results.
#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
pub struct DeployMetadata {
    /// The block hashes of blocks containing the related deploy, along with the results of
    /// executing the related deploy in the context of one or more blocks.
    pub execution_results: HashMap<BlockHash, ExecutionResult>,
}

/// Additional information describing a deploy.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum DeployMetadataExt {
    /// Holds the execution results of a deploy.
    Metadata(DeployMetadata),
    /// Holds the hash and height of the block this deploy was included in.
    BlockInfo(BlockHashAndHeight),
    /// No execution results or block information available.
    Empty,
}

impl Default for DeployMetadataExt {
    fn default() -> Self {
        Self::Empty
    }
}

impl From<DeployMetadata> for DeployMetadataExt {
    fn from(deploy_metadata: DeployMetadata) -> Self {
        Self::Metadata(deploy_metadata)
    }
}

impl From<BlockHashAndHeight> for DeployMetadataExt {
    fn from(deploy_block_info: BlockHashAndHeight) -> Self {
        Self::BlockInfo(deploy_block_info)
    }
}

impl PartialEq<DeployMetadata> for DeployMetadataExt {
    fn eq(&self, other: &DeployMetadata) -> bool {
        match self {
            Self::Metadata(metadata) => *metadata == *other,
            _ => false,
        }
    }
}

impl PartialEq<BlockHashAndHeight> for DeployMetadataExt {
    fn eq(&self, other: &BlockHashAndHeight) -> bool {
        match self {
            Self::BlockInfo(block_hash_and_height) => *block_hash_and_height == *other,
            _ => false,
        }
    }
}

impl ToBytes for Deploy {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.header.to_bytes()?);
        buffer.extend(self.hash.to_bytes()?);
        buffer.extend(self.payment.to_bytes()?);
        buffer.extend(self.session.to_bytes()?);
        buffer.extend(self.approvals.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.header.serialized_length()
            + self.hash.serialized_length()
            + self.payment.serialized_length()
            + self.session.serialized_length()
            + self.approvals.serialized_length()
    }
}

impl FromBytes for Deploy {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (header, remainder) = DeployHeader::from_bytes(bytes)?;
        let (hash, remainder) = DeployHash::from_bytes(remainder)?;
        let (payment, remainder) = ExecutableDeployItem::from_bytes(remainder)?;
        let (session, remainder) = ExecutableDeployItem::from_bytes(remainder)?;
        let (approvals, remainder) = BTreeSet::<Approval>::from_bytes(remainder)?;
        let maybe_valid_deploy = Deploy {
            header,
            hash,
            payment,
            session,
            approvals,
            is_valid: OnceCell::new(),
        };
        Ok((maybe_valid_deploy, remainder))
    }
}

#[cfg(test)]
mod tests {
    use std::{iter, time::Duration};

    use casper_execution_engine::core::engine_state::MAX_PAYMENT_AMOUNT;
    use casper_types::{bytesrepr::Bytes, CLValue};

    use super::*;

    const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;

    #[test]
    fn json_roundtrip() {
        let mut rng = crate::new_rng();
        let deploy = Deploy::random(&mut rng);
        let json_string = serde_json::to_string_pretty(&deploy).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(deploy, decoded);
    }

    #[test]
    fn bincode_roundtrip() {
        let mut rng = crate::new_rng();
        let deploy = Deploy::random(&mut rng);
        let serialized = bincode::serialize(&deploy).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(deploy, deserialized);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let hash = DeployHash(rng.gen::<[u8; Digest::LENGTH]>().into());
        bytesrepr::test_serialization_roundtrip(&hash);

        let deploy = Deploy::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(deploy.header());
        bytesrepr::test_serialization_roundtrip(&deploy);
    }

    fn create_deploy(
        rng: &mut TestRng,
        ttl: TimeDiff,
        dependency_count: usize,
        chain_name: &str,
    ) -> Deploy {
        let secret_key = SecretKey::random(rng);
        let dependencies = iter::repeat_with(|| DeployHash::random(rng))
            .take(dependency_count)
            .collect();
        let transfer_args = {
            let mut transfer_args = RuntimeArgs::new();
            let value =
                CLValue::from_t(U512::from(MAX_PAYMENT_AMOUNT)).expect("should create CLValue");
            transfer_args.insert_cl_value(ARG_AMOUNT, value);
            transfer_args
        };
        Deploy::new(
            Timestamp::now(),
            ttl,
            1,
            dependencies,
            chain_name.to_string(),
            ExecutableDeployItem::ModuleBytes {
                module_bytes: Bytes::new(),
                args: RuntimeArgs::new(),
            },
            ExecutableDeployItem::Transfer {
                args: transfer_args,
            },
            &secret_key,
            None,
        )
    }

    #[test]
    fn is_valid() {
        let mut rng = crate::new_rng();
        let deploy = create_deploy(&mut rng, DeployConfig::default().max_ttl, 0, "net-1");
        assert_eq!(
            deploy.is_valid.get(),
            None,
            "is valid should initially be None"
        );
        deploy.is_valid().expect("should be valid");
        assert_eq!(
            deploy.is_valid.get(),
            Some(&Ok(())),
            "is valid should be true"
        );
    }

    fn check_is_not_valid(invalid_deploy: Deploy, expected_error: DeployConfigurationFailure) {
        assert!(
            invalid_deploy.is_valid.get().is_none(),
            "is valid should initially be None"
        );
        let actual_error = invalid_deploy.is_valid().unwrap_err();

        // Ignore the `error_msg` field of `InvalidApproval` when comparing to expected error, as
        // this makes the test too fragile.  Otherwise expect the actual error should exactly match
        // the expected error.
        match expected_error {
            DeployConfigurationFailure::InvalidApproval {
                index: expected_index,
                ..
            } => match actual_error {
                DeployConfigurationFailure::InvalidApproval {
                    index: actual_index,
                    ..
                } => {
                    assert_eq!(actual_index, expected_index);
                }
                _ => panic!("expected {}, got: {}", expected_error, actual_error),
            },
            _ => {
                assert_eq!(actual_error, expected_error,);
            }
        }

        // The actual error should have been lazily initialized correctly.
        assert_eq!(
            invalid_deploy.is_valid.get(),
            Some(&Err(actual_error)),
            "is valid should now be Some"
        );
    }

    #[test]
    fn not_valid_due_to_invalid_body_hash() {
        let mut rng = crate::new_rng();
        let mut deploy = create_deploy(&mut rng, DeployConfig::default().max_ttl, 0, "net-1");

        deploy.session = ExecutableDeployItem::Transfer {
            args: runtime_args! {
                "amount" => 1
            },
        };
        check_is_not_valid(deploy, DeployConfigurationFailure::InvalidBodyHash);
    }

    #[test]
    fn not_valid_due_to_invalid_deploy_hash() {
        let mut rng = crate::new_rng();
        let mut deploy = create_deploy(&mut rng, DeployConfig::default().max_ttl, 0, "net-1");

        deploy.header.gas_price = 2;
        check_is_not_valid(deploy, DeployConfigurationFailure::InvalidDeployHash);
    }

    #[test]
    fn not_valid_due_to_empty_approvals() {
        let mut rng = crate::new_rng();
        let mut deploy = create_deploy(&mut rng, DeployConfig::default().max_ttl, 0, "net-1");
        deploy.approvals = BTreeSet::new();
        assert!(deploy.approvals.is_empty());
        check_is_not_valid(deploy, DeployConfigurationFailure::EmptyApprovals)
    }

    #[test]
    fn not_valid_due_to_invalid_approval() {
        let mut rng = crate::new_rng();
        let mut deploy = create_deploy(&mut rng, DeployConfig::default().max_ttl, 0, "net-1");

        let deploy2 = Deploy::random(&mut rng);

        deploy.approvals.extend(deploy2.approvals.clone());
        // the expected index for the invalid approval will be the first index at which there is an
        // approval coming from deploy2
        let expected_index = deploy
            .approvals
            .iter()
            .enumerate()
            .find(|(_, approval)| deploy2.approvals.contains(approval))
            .map(|(index, _)| index)
            .unwrap();
        check_is_not_valid(
            deploy,
            DeployConfigurationFailure::InvalidApproval {
                index: expected_index,
                error_msg: String::new(), // This field is ignored in the check.
            },
        );
    }

    #[test]
    fn is_acceptable() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();

        let deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies.into(),
            chain_name,
        );
        deploy
            .is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS)
            .expect("should be acceptable");
    }

    #[test]
    fn not_acceptable_due_to_invalid_chain_name() {
        let mut rng = crate::new_rng();
        let expected_chain_name = "net-1";
        let wrong_chain_name = "net-2".to_string();
        let deploy_config = DeployConfig::default();

        let deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies.into(),
            &wrong_chain_name,
        );

        let expected_error = DeployConfigurationFailure::InvalidChainName {
            expected: expected_chain_name.to_string(),
            got: wrong_chain_name,
        };

        assert_eq!(
            deploy.is_config_compliant(
                expected_chain_name,
                &deploy_config,
                DEFAULT_MAX_ASSOCIATED_KEYS
            ),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_dependencies() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();

        let dependency_count = usize::from(deploy_config.max_dependencies + 1);

        let deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            dependency_count,
            chain_name,
        );

        let expected_error = DeployConfigurationFailure::ExcessiveDependencies {
            max_dependencies: deploy_config.max_dependencies,
            got: dependency_count,
        };

        assert_eq!(
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_ttl() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();

        let ttl = deploy_config.max_ttl + TimeDiff::from(Duration::from_secs(1));

        let deploy = create_deploy(
            &mut rng,
            ttl,
            deploy_config.max_dependencies.into(),
            chain_name,
        );

        let expected_error = DeployConfigurationFailure::ExcessiveTimeToLive {
            max_ttl: deploy_config.max_ttl,
            got: ttl,
        };

        assert_eq!(
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_missing_payment_amount() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: RuntimeArgs::default(),
        };

        // Create an empty session object that is not transfer to ensure
        // that the payment amount is checked.
        let session = ExecutableDeployItem::StoredContractByName {
            name: "".to_string(),
            entry_point: "".to_string(),
            args: Default::default(),
        };

        let mut deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies.into(),
            chain_name,
        );

        deploy.payment = payment;
        deploy.session = session;

        assert_eq!(
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS),
            Err(DeployConfigurationFailure::MissingPaymentAmount)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_mangled_payment_amount() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! {
                "amount" => "mangled-amount"
            },
        };

        // Create an empty session object that is not transfer to ensure
        // that the payment amount is checked.
        let session = ExecutableDeployItem::StoredContractByName {
            name: "".to_string(),
            entry_point: "".to_string(),
            args: Default::default(),
        };

        let mut deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies.into(),
            chain_name,
        );

        deploy.payment = payment;
        deploy.session = session;

        assert_eq!(
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS),
            Err(DeployConfigurationFailure::FailedToParsePaymentAmount)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_payment_amount() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();
        let amount = U512::from(deploy_config.block_gas_limit + 1);

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! {
                "amount" => amount
            },
        };

        // Create an empty session object that is not transfer to ensure
        // that the payment amount is checked.
        let session = ExecutableDeployItem::StoredContractByName {
            name: "".to_string(),
            entry_point: "".to_string(),
            args: Default::default(),
        };

        let mut deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies.into(),
            chain_name,
        );

        deploy.payment = payment;
        deploy.session = session;

        let expected_error = DeployConfigurationFailure::ExceededBlockGasLimit {
            block_gas_limit: deploy_config.block_gas_limit,
            got: amount,
        };

        assert_eq!(
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn transfer_acceptable_regardless_of_excessive_payment_amount() {
        let mut rng = crate::new_rng();
        let secret_key = SecretKey::random(&mut rng);
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();
        let amount = U512::from(deploy_config.block_gas_limit + 1);

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! {
                "amount" => amount
            },
        };

        let transfer_args = {
            let mut transfer_args = RuntimeArgs::new();
            let value =
                CLValue::from_t(U512::from(MAX_PAYMENT_AMOUNT)).expect("should create CLValue");
            transfer_args.insert_cl_value(ARG_AMOUNT, value);
            transfer_args
        };

        let deploy = Deploy::new(
            Timestamp::now(),
            deploy_config.max_ttl,
            1,
            vec![],
            chain_name.to_string(),
            payment,
            ExecutableDeployItem::Transfer {
                args: transfer_args,
            },
            &secret_key,
            None,
        );

        assert_eq!(
            Ok(()),
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS)
        )
    }

    #[test]
    fn not_acceptable_due_to_excessive_approvals() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();
        let deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies as usize,
            chain_name,
        );
        // This test is to ensure a given limit is being checked.
        // Therefore, set the limit to one less than the approvals in the deploy.
        let max_associated_keys = (deploy.approvals.len() - 1) as u32;
        assert_eq!(
            Err(DeployConfigurationFailure::ExcessiveApprovals {
                got: deploy.approvals.len() as u32,
                max_associated_keys: (deploy.approvals.len() - 1) as u32
            }),
            deploy.is_config_compliant(chain_name, &deploy_config, max_associated_keys)
        )
    }

    #[test]
    fn not_acceptable_due_to_missing_transfer_amount() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();
        let mut deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies as usize,
            chain_name,
        );

        let transfer_args = RuntimeArgs::default();
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        deploy.session = session;

        assert_eq!(
            Err(DeployConfigurationFailure::MissingTransferAmount),
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS)
        )
    }

    #[test]
    fn not_acceptable_due_to_mangled_transfer_amount() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();
        let mut deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies as usize,
            chain_name,
        );

        let transfer_args = runtime_args! {
            "amount" => "mangled-amount",
            "source" => PublicKey::random(&mut rng).to_account_hash(),
            "target" => PublicKey::random(&mut rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        deploy.session = session;

        assert_eq!(
            Err(DeployConfigurationFailure::FailedToParseTransferAmount),
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS)
        )
    }

    #[test]
    fn not_acceptable_due_to_insufficient_transfer_amount() {
        let mut rng = crate::new_rng();
        let chain_name = "net-1";
        let deploy_config = DeployConfig::default();
        let mut deploy = create_deploy(
            &mut rng,
            deploy_config.max_ttl,
            deploy_config.max_dependencies as usize,
            chain_name,
        );

        let amount = deploy_config.native_transfer_minimum_motes - 1;
        let insufficient_amount = U512::from(amount);

        let transfer_args = runtime_args! {
            "amount" => insufficient_amount,
            "source" => PublicKey::random(&mut rng).to_account_hash(),
            "target" => PublicKey::random(&mut rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        deploy.session = session;

        assert_eq!(
            Err(DeployConfigurationFailure::InsufficientTransferAmount {
                minimum: U512::from(deploy_config.native_transfer_minimum_motes),
                attempted: insufficient_amount,
            }),
            deploy.is_config_compliant(chain_name, &deploy_config, DEFAULT_MAX_ASSOCIATED_KEYS)
        )
    }
}
