use std::{
    array::TryFromSliceError,
    collections::BTreeSet,
    convert::TryFrom,
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    iter::FromIterator,
};

use datasize::DataSize;
use hex::FromHexError;
use itertools::Itertools;
#[cfg(test)]
use rand::{Rng, RngCore};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;
use tracing::warn;

use casper_execution_engine::core::engine_state::{
    executable_deploy_item::ExecutableDeployItem, DeployItem,
};

use super::{CryptoRngCore, Item, Tag, TimeDiff, Timestamp};
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    components::storage::Value,
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
        Error as CryptoError,
    },
    utils::DisplayIter,
};

const DESER_ERROR_MSG_GENERAL: &str = "failed to deserialize deploy";
const DEPLOY_HASH_MISMATCH_MSG: &str = "deploy hash mismatch";
const DEPLOY_BODY_HASH_MISMATCH_MSG: &str = "deploy body hash mismatch";

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
)]
pub struct DeployHash(Digest);

impl DeployHash {
    /// Constructs a new `DeployHash`.
    pub fn new(hash: Digest) -> Self {
        DeployHash(hash)
    }

    /// Returns the wrapped inner hash.
    pub fn inner(&self) -> &Digest {
        &self.0
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

/// The header portion of a [`Deploy`](struct.Deploy.html).
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
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
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
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

/// A deploy; an item containing a smart contract along with the requester's signature(s).
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct Deploy {
    hash: DeployHash,
    header: DeployHeader,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
    approvals: Vec<Approval>,
}

impl Deploy {
    /// Constructs a new `Deploy`.
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
        rng: &mut dyn CryptoRngCore,
    ) -> Deploy {
        let serialized_body = serialize_body(&payment, &session)
            .unwrap_or_else(|error| panic!("should serialize deploy body: {}", error));
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
        let serialized_header = serialize_header(&header)
            .unwrap_or_else(|error| panic!("should serialize deploy header: {}", error));
        let hash = DeployHash::new(hash::hash(&serialized_header));

        let mut deploy = Deploy {
            hash,
            header,
            payment,
            session,
            approvals: vec![],
        };

        deploy.sign(secret_key, rng);
        deploy
    }

    /// Adds a signature of this deploy's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey, rng: &mut dyn CryptoRngCore) {
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

    /// Convert the `Deploy` to a JSON value.
    pub fn to_json(&self) -> JsonValue {
        json!(self)
    }

    /// Try to convert the JSON value to a `Deploy`.
    pub fn from_json(input: JsonValue) -> Result<Self, Error> {
        let deploy: Deploy = serde_json::from_value(input)?;

        // Serialize and deserialize to run validity checks in deserialization.
        let serialized =
            rmp_serde::to_vec(&deploy).map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
        let _: Deploy = rmp_serde::from_read_ref(&serialized)
            .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;

        Ok(deploy)
    }

    /// Returns the `ExecutableDeployItem` for payment code.
    pub fn payment(&self) -> &ExecutableDeployItem {
        &self.payment
    }

    /// Returns the `ExecutableDeployItem` for session code.
    pub fn session(&self) -> &ExecutableDeployItem {
        &self.session
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        // TODO - make Timestamp deterministic.
        let timestamp = Timestamp::now();
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

fn serialize_header(header: &DeployHeader) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(header)
}

fn deserialize_header(serialized_header: &[u8]) -> Result<DeployHeader, rmp_serde::decode::Error> {
    rmp_serde::from_read_ref(serialized_header)
}

fn serialize_body(
    payment: &ExecutableDeployItem,
    session: &ExecutableDeployItem,
) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(&(&payment, &session))
}

fn deserialize_body(
    serialized_body: &[u8],
) -> Result<(ExecutableDeployItem, ExecutableDeployItem), rmp_serde::decode::Error> {
    rmp_serde::from_read_ref(serialized_body)
}

/// A helper to allow us to derive `Serialize` and `Deserialize` for use with human-readable
/// (de)serializer types.
#[derive(Serialize, Deserialize)]
struct HumanReadableHelper {
    hash: DeployHash,
    header: DeployHeader,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
    approvals: Vec<Approval>,
}

impl Serialize for Deploy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return HumanReadableHelper {
                hash: self.hash,
                header: self.header.clone(),
                payment: self.payment.clone(),
                session: self.session.clone(),
                approvals: self.approvals.clone(),
            }
            .serialize(serializer);
        }

        let deploy_hash = self.hash.as_ref();
        let serialized_header =
            serialize_header(&self.header).map_err(serde::ser::Error::custom)?;
        let serialized_body =
            serialize_body(&self.payment, &self.session).map_err(serde::ser::Error::custom)?;

        let bridging_deploy = BridgingDeploy {
            deploy_hash,
            serialized_header,
            serialized_body,
        };

        (bridging_deploy, &self.approvals).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Deploy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let helper = HumanReadableHelper::deserialize(deserializer)?;
            return Ok(Deploy {
                hash: helper.hash,
                header: helper.header,
                payment: helper.payment,
                session: helper.session,
                approvals: helper.approvals,
            });
        }

        let (bridging_deploy, approvals) =
            <(BridgingDeploy, Vec<Approval>)>::deserialize(deserializer)?;

        let actual_deploy_hash = hash::hash(&bridging_deploy.serialized_header);
        if actual_deploy_hash.as_ref() != bridging_deploy.deploy_hash {
            warn!(
                ?actual_deploy_hash,
                ?bridging_deploy.deploy_hash,
                "{}: {}",
                DESER_ERROR_MSG_GENERAL,
                DEPLOY_HASH_MISMATCH_MSG
            );
            return Err(SerdeError::custom(DEPLOY_HASH_MISMATCH_MSG));
        }

        let header =
            deserialize_header(&bridging_deploy.serialized_header).map_err(SerdeError::custom)?;

        let actual_body_hash = hash::hash(&bridging_deploy.serialized_body);
        if actual_body_hash != header.body_hash {
            warn!(
                ?actual_body_hash,
                ?header.body_hash,
                "{}: {}",
                DESER_ERROR_MSG_GENERAL,
                DEPLOY_BODY_HASH_MISMATCH_MSG
            );
            return Err(SerdeError::custom(DEPLOY_BODY_HASH_MISMATCH_MSG));
        }

        let (payment, session) =
            deserialize_body(&bridging_deploy.serialized_body).map_err(SerdeError::custom)?;

        let deploy = Deploy {
            hash: DeployHash::from(
                Digest::try_from(bridging_deploy.deploy_hash).map_err(SerdeError::custom)?,
            ),
            header,
            payment,
            session,
            approvals,
        };

        for (index, approval) in deploy.approvals.iter().enumerate() {
            if let Err(error) =
                asymmetric_key::verify(&deploy.hash, &approval.signature, &approval.signer)
            {
                let error_msg = format!("failed to verify approval {}", index);
                warn!("{}: {}: {}", DESER_ERROR_MSG_GENERAL, error_msg, error);
                return Err(SerdeError::custom(error_msg));
            }
        }

        Ok(deploy)
    }
}

/// Trait to allow `Deploy`s to be used by the storage component.
impl Value for Deploy {
    type Id = DeployHash;
    type Header = DeployHeader;

    fn id(&self) -> &Self::Id {
        self.id()
    }

    fn header(&self) -> &Self::Header {
        self.header()
    }

    fn take_header(self) -> Self::Header {
        self.take_header()
    }
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
            deploy.id().inner().to_bytes(),
        )
    }
}

/// This is a helper struct to allow for efficient deserialization of a `Deploy` whereby fields
/// required to be serialized for validation are held in a serialized form until validated, and then
/// deserialized into their appropriate types.
#[derive(Serialize, Deserialize)]
struct BridgingDeploy<'a> {
    #[serde(with = "serde_bytes")]
    deploy_hash: &'a [u8],
    #[serde(with = "serde_bytes")]
    serialized_header: Vec<u8>,
    #[serde(with = "serde_bytes")]
    serialized_body: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn json_roundtrip() {
        let mut rng = TestRng::new();
        let deploy = Deploy::random(&mut rng);
        let json_string = deploy.to_json().to_string();
        let json = JsonValue::from_str(json_string.as_str()).unwrap();
        let decoded = Deploy::from_json(json).unwrap();
        assert_eq!(deploy, decoded);
    }

    #[test]
    fn msgpack_roundtrip() {
        let mut rng = TestRng::new();
        let deploy = Deploy::random(&mut rng);
        let serialized = rmp_serde::to_vec(&deploy).unwrap();
        let deserialized = rmp_serde::from_read_ref(&serialized).unwrap();
        assert_eq!(deploy, deserialized);
    }
}
