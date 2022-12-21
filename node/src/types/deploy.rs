// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

mod approval;
mod approvals_hash;
mod deploy_hash;
mod deploy_hash_with_approvals;
mod deploy_header;
mod deploy_or_transfer_hash;
mod deploy_with_finalized_approvals;
mod error;
mod finalized_approvals;
mod footprint;
mod id;
mod legacy_deploy;
mod metadata;

use std::{
    cmp,
    collections::BTreeSet,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

use datasize::DataSize;
use itertools::Itertools;
use once_cell::sync::{Lazy, OnceCell};
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

#[cfg(test)]
use casper_execution_engine::core::engine_state::MAX_PAYMENT;
use casper_execution_engine::core::engine_state::{
    executable_deploy_item::ExecutableDeployItem, DeployItem,
};
use casper_hashing::Digest;
#[cfg(test)]
use casper_types::bytesrepr::Bytes;
#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, runtime_args,
    system::standard_payment::ARG_AMOUNT,
    PublicKey, RuntimeArgs, SecretKey, TimeDiff, Timestamp, U512,
};

use crate::{
    effect::GossipTarget,
    rpcs::docs::DocExample,
    types::{
        chainspec::DeployConfig, EmptyValidationMetadata, FetcherItem, GossiperItem, Item, Tag,
    },
    utils::{ds, DisplayIter},
};
pub use approval::Approval;
pub use approvals_hash::ApprovalsHash;
pub use deploy_hash::DeployHash;
pub(crate) use deploy_hash_with_approvals::DeployHashWithApprovals;
pub use deploy_header::DeployHeader;
pub use deploy_or_transfer_hash::DeployOrTransferHash;
pub(crate) use deploy_with_finalized_approvals::DeployWithFinalizedApprovals;
pub use error::{DeployConfigurationFailure, Error as DeployError, ExcessiveSizeError};
pub(crate) use finalized_approvals::FinalizedApprovals;
pub(crate) use footprint::Footprint as DeployFootprint;
pub use id::Id as DeployId;
pub(crate) use legacy_deploy::LegacyDeploy;
pub(crate) use metadata::{Metadata as DeployMetadata, MetadataExt as DeployMetadataExt};

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
    let header = DeployHeader::new(
        PublicKey::from(secret_key),
        *Timestamp::doc_example(),
        TimeDiff::from_seconds(3_600),
        1,
        body_hash,
        vec![DeployHash::new(Digest::from([1u8; Digest::LENGTH]))],
        String::from("casper-example"),
    );
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
        let header = DeployHeader::new(
            account,
            timestamp,
            ttl,
            gas_price,
            body_hash,
            dependencies,
            chain_name,
        );
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
    pub fn hash(&self) -> &DeployHash {
        &self.hash
    }

    /// Returns the `ApprovalsHash` of this `Deploy`'s approvals.
    pub fn approvals_hash(&self) -> Result<ApprovalsHash, bytesrepr::Error> {
        ApprovalsHash::compute(&self.approvals)
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

    /// Returns the hash of this deploy wrapped in `DeployOrTransferHash`.
    pub fn deploy_or_transfer_hash(&self) -> DeployOrTransferHash {
        if self.session.is_transfer() {
            DeployOrTransferHash::Transfer(self.hash)
        } else {
            DeployOrTransferHash::Deploy(self.hash)
        }
    }

    pub(crate) fn with_approvals(mut self, approvals: BTreeSet<Approval>) -> Self {
        self.approvals = approvals;
        self
    }

    /// Returns the `DeployFootprint`.
    pub(crate) fn footprint(&self) -> Result<DeployFootprint, DeployError> {
        let header = self.header().clone();
        let gas_estimate = match self.payment().payment_amount(header.gas_price()) {
            Some(gas) => gas,
            None => {
                return Err(DeployError::InvalidPayment);
            }
        };
        let size_estimate = self.serialized_length();
        let is_transfer = self.session.is_transfer();
        Ok(DeployFootprint {
            header,
            gas_estimate,
            size_estimate,
            is_transfer,
        })
    }

    /// Returns true if the serialized size of the deploy is not greater than `max_deploy_size`.
    fn is_valid_size(&self, max_deploy_size: u32) -> Result<(), ExcessiveSizeError> {
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
        if body_hash != *self.header.body_hash() {
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
                deploy_hash = %self.hash(),
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
                deploy_hash = %self.hash(),
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
                deploy_hash = %self.hash(),
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
                deploy_hash = %self.hash(),
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

impl ToBytes for Deploy {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.header.write_bytes(writer)?;
        self.hash.write_bytes(writer)?;
        self.payment.write_bytes(writer)?;
        self.session.write_bytes(writer)?;
        self.approvals.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
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

impl Item for Deploy {
    type Id = DeployId;

    fn id(&self) -> Self::Id {
        let deploy_hash = *self.hash();
        let approvals_hash = self.approvals_hash().unwrap_or_else(|error| {
            error!(%error, "failed to serialize approvals");
            ApprovalsHash::from(Digest::default())
        });
        DeployId::new(deploy_hash, approvals_hash)
    }
}

impl FetcherItem for Deploy {
    type ValidationError = DeployConfigurationFailure;
    type ValidationMetadata = EmptyValidationMetadata;
    const TAG: Tag = Tag::Deploy;

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.is_valid()
    }
}

impl GossiperItem for Deploy {
    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    fn target(&self) -> GossipTarget {
        GossipTarget::All
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

impl DocExample for Deploy {
    fn doc_example() -> &'static Self {
        &*DEPLOY
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
            casper_types::DeployHash::new(deploy.hash().inner().value()),
        )
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

/// Computationally expensive validity check for a given deploy instance, including
/// asymmetric_key signing verification.
fn validate_deploy(deploy: &Deploy) -> Result<(), DeployConfigurationFailure> {
    if deploy.approvals.is_empty() {
        warn!(?deploy, "deploy has no approvals");
        return Err(DeployConfigurationFailure::EmptyApprovals);
    }

    deploy.has_valid_hash()?;

    for (index, approval) in deploy.approvals.iter().enumerate() {
        if let Err(error) = crypto::verify(&deploy.hash, approval.signature(), approval.signer()) {
            warn!(?deploy, "failed to verify approval {}: {}", index, error);
            return Err(DeployConfigurationFailure::InvalidApproval {
                index,
                error_msg: error.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(any(feature = "testing", test))]
impl Deploy {
    /// Returns a random deploy.
    pub fn random(rng: &mut TestRng) -> Self {
        let timestamp = Timestamp::random(rng);
        let ttl = TimeDiff::from_seconds(rng.gen_range(60..300));
        Deploy::random_with_timestamp_and_ttl(rng, timestamp, ttl)
    }

    /// Returns a random deploy but using the specified `timestamp` and `ttl`.
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
}

#[cfg(test)]
impl Deploy {
    /// Turns `self` into an invalid deploy by clearing the `chain_name`, invalidating the deploy
    /// hash.
    pub(crate) fn invalidate(&mut self) {
        self.header.invalidate();
    }

    /// Returns a random deploy for a native transfer.
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
            deploy.header.ttl(),
            deploy.header.gas_price(),
            deploy.header.dependencies().clone(),
            deploy.header.chain_name().to_string(),
            payment,
            session,
            &secret_key,
            None,
        )
    }

    /// Returns a random deploy for a native transfer with no dependencies.
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
            deploy.header.ttl(),
            deploy.header.gas_price(),
            vec![],
            deploy.header.chain_name().to_string(),
            payment,
            session,
            &secret_key,
            None,
        )
    }

    /// Returns a random invalid deploy without a payment amount specified.
    pub(crate) fn random_without_payment_amount(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: RuntimeArgs::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid deploy with an invalid value for the payment amount.
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

    /// Returns a random deploy with custom payment specified as a stored contract by name.
    pub(crate) fn random_with_valid_custom_payment_contract_by_name(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByName {
            name: "Test".to_string(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid deploy with custom payment specified as a stored contract by hash,
    /// but missing the runtime args.
    pub(crate) fn random_with_missing_payment_contract_by_hash(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid deploy with custom payment specified as a stored contract by hash,
    /// but calling an invalid entry point.
    pub(crate) fn random_with_missing_entry_point_in_payment_contract(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random deploy with custom payment specified as a stored versioned contract by
    /// name.
    pub(crate) fn random_with_valid_custom_payment_package_by_name(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByName {
            name: "Test".to_string(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid deploy with custom payment specified as a stored versioned contract
    /// by hash, but missing the runtime args.
    pub(crate) fn random_with_missing_payment_package_by_hash(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: Default::default(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid deploy with custom payment specified as a stored versioned contract
    /// by hash, but calling an invalid entry point.
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

    /// Returns a random deploy with custom session specified as a stored contract by name.
    pub(crate) fn random_with_valid_session_contract_by_name(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByName {
            name: "Test".to_string(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid deploy with custom session specified as a stored contract by hash,
    /// but missing the runtime args.
    pub(crate) fn random_with_missing_session_contract_by_hash(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: Default::default(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid deploy with custom session specified as a stored contract by hash,
    /// but calling an invalid entry point.
    pub(crate) fn random_with_missing_entry_point_in_session_contract(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random deploy with custom session specified as a stored versioned contract by
    /// name.
    pub(crate) fn random_with_valid_session_package_by_name(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByName {
            name: "Test".to_string(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid deploy with custom session specified as a stored versioned contract
    /// by hash, but missing the runtime args.
    pub(crate) fn random_with_missing_session_package_by_hash(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: Default::default(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid deploy with custom session specified as a stored versioned contract
    /// by hash, but calling an invalid entry point.
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

    /// Returns a random invalid transfer deploy with the "target" runtime arg missing.
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

    /// Returns a random invalid transfer deploy with the "amount" runtime arg missing.
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

    /// Returns a random invalid transfer deploy with an invalid "amount" runtime arg.
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

    /// Returns a random invalid deploy with empty session bytes.
    pub(crate) fn random_with_empty_session_module_bytes(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid deploy with an expired TTL.
    pub(crate) fn random_expired_deploy(rng: &mut TestRng) -> Self {
        let deploy = Self::random_valid_native_transfer(rng);
        let secret_key = SecretKey::random(rng);

        Deploy::new(
            Timestamp::zero(),
            TimeDiff::from_seconds(1u32),
            deploy.header.gas_price(),
            deploy.header.dependencies().clone(),
            deploy.header.chain_name().to_string(),
            deploy.payment,
            deploy.session,
            &secret_key,
            None,
        )
    }

    /// Returns a random deploy with native transfer as payment code.
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
            deploy.header.timestamp(),
            deploy.header.ttl(),
            deploy.header.gas_price(),
            deploy.header.dependencies().clone(),
            deploy.header.chain_name().to_string(),
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
            deploy.header.timestamp(),
            deploy.header.ttl(),
            deploy.header.gas_price(),
            deploy.header.dependencies().clone(),
            deploy.header.chain_name().to_string(),
            deploy.payment,
            session,
            &secret_key,
            None,
        )
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

        // deploy.header.gas_price = 2;
        deploy.invalidate();
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
