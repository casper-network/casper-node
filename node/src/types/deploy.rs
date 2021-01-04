// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    array::TryFromSliceError,
    collections::{BTreeSet, HashMap},
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    iter::FromIterator,
};

use datasize::DataSize;
use hex::FromHexError;
use itertools::Itertools;
use once_cell::sync::Lazy;
#[cfg(test)]
use rand::{Rng, RngCore};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

use casper_execution_engine::{
    core::engine_state::{executable_deploy_item::ExecutableDeployItem, DeployItem},
    shared::motes::Motes,
};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    standard_payment::ARG_AMOUNT,
    ExecutionResult, U512,
};

use super::{BlockHash, Item, Tag, TimeDiff, Timestamp};
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    components::{block_proposer::DeployType, chainspec_loader::DeployConfig},
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
        Error as CryptoError,
    },
    rpcs::docs::DocExample,
    utils::DisplayIter,
    NodeRng,
};
use num_traits::Zero;

static DEPLOY: Lazy<Deploy> = Lazy::new(|| {
    let payment = ExecutableDeployItem::StoredContractByName {
        name: String::from("casper-example"),
        entry_point: String::from("example-entry-point"),
        args: vec![1, 1].into(),
    };
    let session = ExecutableDeployItem::Transfer {
        args: vec![2, 2].into(),
    };
    let serialized_body = serialize_body(&payment, &session);
    let body_hash = hash::hash(&serialized_body);

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
    let hash = DeployHash::new(hash::hash(&serialized_header));

    let signature = Signature::from_hex(
        "012dbf03817a51794a8e19e0724884075e6d1fbec326b766ecfa6658b41f81290da85e23b24e88b1c8d976\
            1185c961daee1adab0649912a6477bcd2e69bd91bd08"
            .as_bytes(),
    )
    .unwrap();
    let approval = Approval {
        signer: PublicKey::from(secret_key),
        signature,
    };

    Deploy {
        hash,
        header,
        payment,
        session,
        approvals: vec![approval],
        is_valid: None,
    }
});

/// Error returned from constructing or validating a `Deploy`.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding to JSON.
    #[error("encoding to JSON: {0}")]
    EncodeToJson(#[from] serde_json::Error),

    /// Error while decoding from JSON.
    #[error("decoding from JSON: {0}")]
    DecodeFromJson(Box<dyn StdError>),

    /// Approval at specified index does not exist.
    #[error("approval at index {0} does not exist")]
    NoSuchApproval(usize),

    /// Failed to verify an approval.
    #[error("failed to verify approval {index}: {error}")]
    FailedVerification {
        /// The index of the failed approval.
        index: usize,
        /// The verification error.
        error: CryptoError,
    },

    /// Failed to get "amount" from `payment()`'s runtime args.
    #[error("invalid payment: missing \"amount\" arg")]
    InvalidPayment,
}

impl From<FromHexError> for Error {
    fn from(error: FromHexError) -> Self {
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
        let hash = Digest::random(rng);
        DeployHash(hash)
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
        let lifespan = self.timestamp + self.ttl;
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
}

impl DeployHeader {
    /// Returns the timestamp of when the deploy expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp + self.ttl
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
    /// Returns the public key of the approval's signer.
    pub fn signer(&self) -> &PublicKey {
        &self.signer
    }

    /// Returns the approval signature.
    pub fn signature(&self) -> &Signature {
        &self.signature
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

/// A deploy; an item containing a smart contract along with the requester's signature(s).
#[derive(
    Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Deploy {
    hash: DeployHash,
    header: DeployHeader,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
    approvals: Vec<Approval>,
    #[serde(skip)]
    is_valid: Option<bool>,
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
        rng: &mut NodeRng,
    ) -> Deploy {
        let serialized_body = serialize_body(&payment, &session);
        let body_hash = hash::hash(&serialized_body);

        let account = PublicKey::from(secret_key);
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
        let hash = DeployHash::new(hash::hash(&serialized_header));

        let mut deploy = Deploy {
            hash,
            header,
            payment,
            session,
            approvals: vec![],
            is_valid: None,
        };

        deploy.sign(secret_key, rng);
        deploy
    }

    /// Adds a signature of this deploy's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey, rng: &mut NodeRng) {
        let signer = PublicKey::from(secret_key);
        let signature = asymmetric_key::sign(&self.hash, secret_key, &signer, rng);
        let approval = Approval { signer, signature };
        self.approvals.push(approval);
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
    pub fn approvals(&self) -> &[Approval] {
        &self.approvals
    }

    /// Returns the `DeployType`.
    pub fn deploy_type(&self) -> Result<DeployType, Error> {
        let header = self.header().clone();
        let size = self.serialized_length();
        if self.session().is_transfer() {
            // TODO: we need a non-zero value constant for wasm-less transfer cost.
            let payment_amount = Motes::zero();
            Ok(DeployType::Transfer {
                header,
                payment_amount,
                size,
            })
        } else {
            let payment_item = self.payment().clone();
            let payment_amount = {
                // In the happy path for a payment we expect:
                // - args to exist
                // - contain "amount"
                // - be a valid U512 value.
                let args = payment_item
                    .into_runtime_args()
                    .map_err(|_| Error::InvalidPayment)?;
                let value = args.get(ARG_AMOUNT).ok_or(Error::InvalidPayment)?;
                let value = value
                    .clone()
                    .into_t::<U512>()
                    .map_err(|_| Error::InvalidPayment)?;
                Motes::new(value)
            };
            Ok(DeployType::Other {
                header,
                payment_amount,
                size,
            })
        }
    }

    /// Returns true if and only if:
    ///   * the deploy hash is correct (should be the hash of the header), and
    ///   * the body hash is correct (should be the hash of the body), and
    ///   * all approvals are valid signatures of the deploy hash
    ///
    /// Note: this is a relatively expensive operation, requiring re-serialization of the deploy,
    ///       hashing, and signature checking, so should be called as infrequently as possible.
    pub fn is_valid(&mut self) -> bool {
        match self.is_valid {
            None => {
                let validity = validate_deploy(self);
                self.is_valid = Some(validity);
                validity
            }
            Some(validity) => validity,
        }
    }

    /// Returns true if and only if:
    ///   * the chain_name is correct,
    ///   * the configured parameters are complied with,
    ///   * the deploy is valid
    ///
    /// Note: if everything else checks out, calls the computationally expensive `is_valid`
    /// method.
    pub fn is_acceptable(&mut self, chain_name: String, config: DeployConfig) -> bool {
        let header = self.header();
        if header.chain_name() != chain_name {
            warn!(
                deploy_hash = %self.id(),
                deploy_header = %header,
                chain_name = %header.chain_name(),
                "invalid chain identifier"
            );
            return false;
        }

        if header.dependencies().len() > config.max_dependencies as usize {
            warn!(
                deploy_hash = %self.id(),
                deploy_header = %header,
                max_dependencies = %config.max_dependencies,
                "deploy dependency ceiling exceeded"
            );
            return false;
        }

        if header.ttl() > config.max_ttl {
            warn!(
                deploy_hash = %self.id(),
                deploy_header = %header,
                max_ttl = %config.max_ttl,
                "deploy ttl excessive"
            );
            return false;
        }

        // TODO - check if there is more that can be validated here.

        self.is_valid()
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let timestamp = Timestamp::random(rng);
        let ttl = TimeDiff::from(rng.gen_range(60_000, 3_600_000));
        let gas_price = rng.gen_range(1, 100);

        let dependencies = vec![
            DeployHash::new(hash::hash(rng.next_u64().to_le_bytes())),
            DeployHash::new(hash::hash(rng.next_u64().to_le_bytes())),
            DeployHash::new(hash::hash(rng.next_u64().to_le_bytes())),
        ];
        let chain_name = String::from("casper-example");

        let payment = rng.gen();
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
            rng,
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
fn validate_deploy(deploy: &Deploy) -> bool {
    let serialized_body = serialize_body(&deploy.payment, &deploy.session);
    let body_hash = hash::hash(&serialized_body);
    if body_hash != deploy.header.body_hash {
        warn!(?deploy, ?body_hash, "invalid deploy body hash");
        return false;
    }

    let serialized_header = serialize_header(&deploy.header);
    let hash = DeployHash::new(hash::hash(&serialized_header));
    if hash != deploy.hash {
        warn!(?deploy, ?hash, "invalid deploy hash");
        return false;
    }

    // We don't need to check for an empty set here. EE checks that the correct number and weight of
    // signatures are provided when executing the deploy, so all we need to do here is check that
    // any provided signatures are valid.
    for (index, approval) in deploy.approvals.iter().enumerate() {
        if let Err(error) =
            asymmetric_key::verify(&deploy.hash, &approval.signature, &approval.signer)
        {
            warn!(?deploy, "failed to verify approval {}: {}", index, error);
            return false;
        }
    }

    true
}

impl Item for Deploy {
    type Id = DeployHash;

    const TAG: Tag = Tag::Deploy;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn id(&self) -> Self::Id {
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
        let account_hash = deploy.header().account().to_account_hash();
        DeployItem::new(
            account_hash,
            deploy.session().clone(),
            deploy.payment().clone(),
            deploy.header().gas_price(),
            BTreeSet::from_iter(vec![account_hash]),
            casper_types::DeployHash::new(deploy.id().inner().to_array()),
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
        let (approvals, remainder) = Vec::<Approval>::from_bytes(remainder)?;
        let maybe_valid_deploy = Deploy {
            header,
            hash,
            payment,
            session,
            approvals,
            is_valid: None,
        };
        Ok((maybe_valid_deploy, remainder))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use casper_types::bytesrepr::Bytes;

    use super::*;

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
        let hash = DeployHash(Digest::random(&mut rng));
        bytesrepr::test_serialization_roundtrip(&hash);

        let deploy = Deploy::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(deploy.header());
        bytesrepr::test_serialization_roundtrip(&deploy);
    }

    #[test]
    fn is_valid() {
        let mut rng = crate::new_rng();
        let mut deploy = Deploy::random(&mut rng);
        assert_eq!(deploy.is_valid, None, "is valid should initially be None");
        assert!(deploy.is_valid());
        assert_eq!(deploy.is_valid, Some(true), "is valid should be true");
    }

    #[test]
    fn is_not_valid() {
        let mut deploy = Deploy::new(
            Timestamp::zero(),
            TimeDiff::from(Duration::default()),
            0,
            vec![],
            String::default(),
            ExecutableDeployItem::ModuleBytes {
                module_bytes: Bytes::new(),
                args: Bytes::new(),
            },
            ExecutableDeployItem::Transfer { args: Bytes::new() },
            &SecretKey::generate_ed25519(),
            &mut crate::new_rng(),
        );
        deploy.header.gas_price = 1;
        assert_eq!(deploy.is_valid, None, "is valid should initially be None");
        assert!(!deploy.is_valid(), "should not be valid");
        assert_eq!(deploy.is_valid, Some(false), "is valid should be false");
    }
}
