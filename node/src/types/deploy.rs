use std::{
    array::TryFromSliceError,
    collections::BTreeSet,
    convert::TryFrom,
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    iter::FromIterator,
};

use hex::FromHexError;
use itertools::Itertools;
#[cfg(test)]
use rand::RngCore;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use tracing::warn;

use casper_execution_engine::core::engine_state::{
    executable_deploy_item::ExecutableDeployItem, DeployItem,
};

use super::{Item, Tag, TimeDiff, Timestamp};
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
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
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
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
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
    pub fn new<R: Rng + CryptoRng + ?Sized>(
        timestamp: Timestamp,
        ttl: TimeDiff,
        gas_price: u64,
        dependencies: Vec<DeployHash>,
        chain_name: String,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
        secret_key: &SecretKey,
        rng: &mut R,
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
    pub fn sign<R: Rng + CryptoRng + ?Sized>(&mut self, secret_key: &SecretKey, rng: &mut R) {
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

    /// Try to convert the `Deploy` to JSON-encoded string.
    pub fn to_json(&self) -> Result<String, Error> {
        let json = json::JsonDeploy::from(self);
        Ok(serde_json::to_string(&json)?)
    }

    /// Try to convert the JSON-encoded string to a `Deploy`.
    pub fn from_json(input: &str) -> Result<Self, Error> {
        let json: json::JsonDeploy = serde_json::from_str(input)?;
        Deploy::try_from(json)
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

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: hash::hash(rng.next_u64().to_le_bytes()).as_ref().to_vec(),
            args: hash::hash(rng.next_u64().to_le_bytes()).as_ref().to_vec(),
        };
        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: hash::hash(rng.next_u64().to_le_bytes()).as_ref().to_vec(),
            args: hash::hash(rng.next_u64().to_le_bytes()).as_ref().to_vec(),
        };

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

impl Serialize for Deploy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
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
            return Err(serde::de::Error::custom(DEPLOY_HASH_MISMATCH_MSG));
        }

        let header = deserialize_header(&bridging_deploy.serialized_header)
            .map_err(serde::de::Error::custom)?;

        let actual_body_hash = hash::hash(&bridging_deploy.serialized_body);
        if actual_body_hash != header.body_hash {
            warn!(
                ?actual_body_hash,
                ?header.body_hash,
                "{}: {}",
                DESER_ERROR_MSG_GENERAL,
                DEPLOY_BODY_HASH_MISMATCH_MSG
            );
            return Err(serde::de::Error::custom(DEPLOY_BODY_HASH_MISMATCH_MSG));
        }

        let (payment, session) =
            deserialize_body(&bridging_deploy.serialized_body).map_err(serde::de::Error::custom)?;

        let deploy = Deploy {
            hash: DeployHash::from(
                Digest::try_from(bridging_deploy.deploy_hash).map_err(serde::de::Error::custom)?,
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
                return Err(serde::de::Error::custom(error_msg));
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

/// This module provides structs which map to the main deploy types, but which are suitable for
/// encoding to and decoding from JSON.  For all fields with binary data, this is converted to/from
/// hex strings.
mod json {
    use std::convert::{TryFrom, TryInto};

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        crypto::{
            asymmetric_key::{PublicKey, Signature},
            hash::Digest,
        },
        types::{TimeDiff, Timestamp},
    };
    use casper_types::ContractVersion;

    #[derive(Serialize, Deserialize)]
    struct JsonDeployHash(String);

    #[derive(Serialize, Deserialize)]
    enum JsonExecutableDeployItem {
        ModuleBytes {
            module_bytes: String,
            args: String,
        },
        StoredContractByHash {
            hash: String,
            entry_point: String,
            args: String,
        },
        StoredContractByName {
            name: String,
            entry_point: String,
            args: String,
        },
        StoredVersionedContractByName {
            name: String,
            version: Option<ContractVersion>, // defaults to highest enabled version
            entry_point: String,
            args: String,
        },
        StoredVersionedContractByHash {
            hash: String,
            version: Option<ContractVersion>, // defaults to highest enabled version
            entry_point: String,
            args: String,
        },
        Transfer {
            args: String,
        },
    }

    impl From<&DeployHash> for JsonDeployHash {
        fn from(hash: &DeployHash) -> Self {
            JsonDeployHash(hex::encode(hash.0))
        }
    }

    impl TryFrom<JsonDeployHash> for DeployHash {
        type Error = Error;

        fn try_from(hash: JsonDeployHash) -> Result<Self, Self::Error> {
            let hash = Digest::from_hex(&hash.0)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
            Ok(DeployHash(hash))
        }
    }

    impl From<ExecutableDeployItem> for JsonExecutableDeployItem {
        fn from(executable_deploy_item: ExecutableDeployItem) -> Self {
            match executable_deploy_item {
                ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                    JsonExecutableDeployItem::ModuleBytes {
                        module_bytes: hex::encode(&module_bytes),
                        args: hex::encode(&args),
                    }
                }
                ExecutableDeployItem::StoredContractByHash {
                    hash,
                    entry_point,
                    args,
                } => JsonExecutableDeployItem::StoredContractByHash {
                    hash: hex::encode(&hash),
                    entry_point,
                    args: hex::encode(&args),
                },
                ExecutableDeployItem::StoredContractByName {
                    name,
                    entry_point,
                    args,
                } => JsonExecutableDeployItem::StoredContractByName {
                    name,
                    entry_point,
                    args: hex::encode(&args),
                },
                ExecutableDeployItem::StoredVersionedContractByName {
                    name,
                    version,
                    entry_point,
                    args,
                } => JsonExecutableDeployItem::StoredVersionedContractByName {
                    name,
                    version,
                    entry_point,
                    args: hex::encode(&args),
                },
                ExecutableDeployItem::StoredVersionedContractByHash {
                    hash,
                    version,
                    entry_point,
                    args,
                } => JsonExecutableDeployItem::StoredVersionedContractByHash {
                    hash: hex::encode(&hash),
                    version,
                    entry_point,
                    args: hex::encode(&args),
                },
                ExecutableDeployItem::Transfer { args } => JsonExecutableDeployItem::Transfer {
                    args: hex::encode(&args),
                },
            }
        }
    }

    impl TryFrom<JsonExecutableDeployItem> for ExecutableDeployItem {
        type Error = Error;
        fn try_from(value: JsonExecutableDeployItem) -> Result<Self, Self::Error> {
            let executable_deploy_item = match value {
                JsonExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                    ExecutableDeployItem::ModuleBytes {
                        module_bytes: hex::decode(&module_bytes)?,
                        args: hex::decode(&args)?,
                    }
                }
                JsonExecutableDeployItem::StoredContractByHash {
                    hash,
                    entry_point,
                    args,
                } => ExecutableDeployItem::StoredContractByHash {
                    hash: hex::decode(&hash)?.as_slice().try_into()?,
                    entry_point,
                    args: hex::decode(&args)?,
                },
                JsonExecutableDeployItem::StoredContractByName {
                    name,
                    entry_point,
                    args,
                } => ExecutableDeployItem::StoredContractByName {
                    name,
                    entry_point,
                    args: hex::decode(&args)?,
                },
                JsonExecutableDeployItem::StoredVersionedContractByName {
                    name,
                    version,
                    entry_point,
                    args,
                } => ExecutableDeployItem::StoredVersionedContractByName {
                    name,
                    version,
                    entry_point,
                    args: hex::decode(&args)?,
                },
                JsonExecutableDeployItem::StoredVersionedContractByHash {
                    hash,
                    version,
                    entry_point,
                    args,
                } => ExecutableDeployItem::StoredVersionedContractByHash {
                    hash: hex::decode(&hash)?.as_slice().try_into()?,
                    version,
                    entry_point,
                    args: hex::decode(&args)?,
                },
                JsonExecutableDeployItem::Transfer { args } => ExecutableDeployItem::Transfer {
                    args: hex::decode(&args)?,
                },
            };
            Ok(executable_deploy_item)
        }
    }

    #[derive(Serialize, Deserialize)]
    struct JsonDeployHeader {
        account: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        gas_price: u64,
        body_hash: String,
        dependencies: Vec<JsonDeployHash>,
        chain_name: String,
    }

    impl From<&DeployHeader> for JsonDeployHeader {
        fn from(header: &DeployHeader) -> Self {
            JsonDeployHeader {
                account: header.account.to_hex(),
                timestamp: header.timestamp,
                ttl: header.ttl,
                gas_price: header.gas_price,
                body_hash: hex::encode(header.body_hash),
                dependencies: header.dependencies.iter().map(Into::into).collect(),
                chain_name: header.chain_name.clone(),
            }
        }
    }

    impl TryFrom<JsonDeployHeader> for DeployHeader {
        type Error = Error;

        fn try_from(header: JsonDeployHeader) -> Result<Self, Self::Error> {
            let account = PublicKey::from_hex(&header.account)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;

            let body_hash = Digest::from_hex(&header.body_hash)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;

            let mut dependencies = vec![];
            for dep in header.dependencies.into_iter() {
                let hash = dep.try_into()?;
                dependencies.push(hash);
            }

            Ok(DeployHeader {
                account,
                timestamp: header.timestamp,
                ttl: header.ttl,
                gas_price: header.gas_price,
                body_hash,
                dependencies,
                chain_name: header.chain_name,
            })
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct JsonApproval {
        signer: String,
        signature: String,
    }

    impl From<&Approval> for JsonApproval {
        fn from(approval: &Approval) -> Self {
            JsonApproval {
                signer: approval.signer.to_hex(),
                signature: approval.signature.to_hex(),
            }
        }
    }

    impl TryFrom<JsonApproval> for Approval {
        type Error = Error;

        fn try_from(approval: JsonApproval) -> Result<Self, Self::Error> {
            let signer = PublicKey::from_hex(&approval.signer)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
            let signature = Signature::from_hex(&approval.signature)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
            Ok(Approval { signer, signature })
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct JsonDeploy {
        hash: JsonDeployHash,
        header: JsonDeployHeader,
        payment: JsonExecutableDeployItem,
        session: JsonExecutableDeployItem,
        approvals: Vec<JsonApproval>,
    }

    impl From<&Deploy> for JsonDeploy {
        fn from(deploy: &Deploy) -> Self {
            JsonDeploy {
                hash: (&deploy.hash).into(),
                header: (&deploy.header).into(),
                payment: deploy.payment.clone().into(),
                session: deploy.session.clone().into(),
                approvals: deploy.approvals.iter().map(JsonApproval::from).collect(),
            }
        }
    }

    impl TryFrom<JsonDeploy> for Deploy {
        type Error = Error;

        fn try_from(deploy: JsonDeploy) -> Result<Self, Self::Error> {
            let mut approvals = vec![];
            for json_approval in deploy.approvals.into_iter() {
                let approval = Approval::try_from(json_approval)?;
                approvals.push(approval);
            }
            Ok(Deploy {
                hash: deploy.hash.try_into()?,
                header: deploy.header.try_into()?,
                payment: deploy.payment.try_into()?,
                session: deploy.session.try_into()?,
                approvals,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn json_roundtrip() {
        let mut rng = TestRng::new();
        let deploy = Deploy::random(&mut rng);
        let json = deploy.to_json().unwrap();
        let decoded = Deploy::from_json(&json).unwrap();
        assert_eq!(deploy, decoded);
    }

    #[test]
    fn serde_roundtrip() {
        let mut rng = TestRng::new();
        let deploy = Deploy::random(&mut rng);
        let serialized = rmp_serde::to_vec(&deploy).unwrap();
        let deserialized = rmp_serde::from_read_ref(&serialized).unwrap();
        assert_eq!(deploy, deserialized);
    }
}
