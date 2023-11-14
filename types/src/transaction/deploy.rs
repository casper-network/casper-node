mod deploy_approval;
mod deploy_approvals_hash;
#[cfg(any(feature = "std", test))]
mod deploy_builder;
mod deploy_footprint;
mod deploy_hash;
mod deploy_header;
mod deploy_id;
mod error;
mod executable_deploy_item;
mod finalized_deploy_approvals;

use alloc::{collections::BTreeSet, vec::Vec};
use core::{
    cmp,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
pub use finalized_deploy_approvals::FinalizedDeployApprovals;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(any(feature = "std", test))]
use {
    super::{InitiatorAddr, InitiatorAddrAndSecretKey},
    itertools::Itertools,
    serde::{Deserialize, Serialize},
};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use {
    crate::{
        bytesrepr::Bytes,
        system::auction::{
            ARG_AMOUNT as ARG_AUCTION_AMOUNT, ARG_DELEGATOR, ARG_NEW_VALIDATOR,
            ARG_PUBLIC_KEY as ARG_AUCTION_PUBLIC_KEY, ARG_VALIDATOR, METHOD_DELEGATE,
            METHOD_REDELEGATE, METHOD_UNDELEGATE, METHOD_WITHDRAW_BID,
        },
        AddressableEntityHash,
        {system::mint::ARG_AMOUNT, TransactionConfig, U512},
        {testing::TestRng, DEFAULT_MAX_PAYMENT_MOTES, DEFAULT_MIN_TRANSFER_MOTES},
    },
    rand::{Rng, RngCore},
    tracing::{debug, warn},
};
#[cfg(feature = "json-schema")]
use {once_cell::sync::Lazy, schemars::JsonSchema};

#[cfg(any(
    all(feature = "std", feature = "testing"),
    feature = "json-schema",
    test
))]
use crate::runtime_args;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::RuntimeArgs;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, Digest, DisplayIter, PublicKey, SecretKey, TimeDiff, Timestamp,
};

pub use deploy_approval::DeployApproval;
pub use deploy_approvals_hash::DeployApprovalsHash;
#[cfg(any(feature = "std", test))]
pub use deploy_builder::{DeployBuilder, DeployBuilderError};
pub use deploy_footprint::DeployFootprint;
pub use deploy_hash::DeployHash;
pub use deploy_header::DeployHeader;
pub use deploy_id::DeployId;
pub use error::{
    DecodeFromJsonError as DeployDecodeFromJsonError, DeployConfigFailure, Error as DeployError,
    ExcessiveSizeError as DeployExcessiveSizeError,
};
pub use executable_deploy_item::{
    ExecutableDeployItem, ExecutableDeployItemIdentifier, TransferTarget,
};

#[cfg(feature = "json-schema")]
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
    let body_hash = Digest::hash(serialized_body);

    let secret_key = SecretKey::example();
    let timestamp = *Timestamp::example();
    let header = DeployHeader::new(
        PublicKey::from(secret_key),
        timestamp,
        TimeDiff::from_seconds(3_600),
        1,
        body_hash,
        vec![DeployHash::new(Digest::from([1u8; Digest::LENGTH]))],
        String::from("casper-example"),
    );
    let serialized_header = serialize_header(&header);
    let hash = DeployHash::new(Digest::hash(serialized_header));

    let mut approvals = BTreeSet::new();
    let approval = DeployApproval::create(&hash, secret_key);
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

/// A signed smart contract.
///
/// To construct a new `Deploy`, use a [`DeployBuilder`].
#[derive(Clone, Eq, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A signed smart contract.")
)]
pub struct Deploy {
    hash: DeployHash,
    header: DeployHeader,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
    approvals: BTreeSet<DeployApproval>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    is_valid: OnceCell<Result<(), DeployConfigFailure>>,
}

impl Deploy {
    /// Called by the `DeployBuilder` to construct a new `Deploy`.
    #[cfg(any(feature = "std", test))]
    #[allow(clippy::too_many_arguments)]
    fn build(
        timestamp: Timestamp,
        ttl: TimeDiff,
        gas_price: u64,
        dependencies: Vec<DeployHash>,
        chain_name: String,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
        initiator_addr_and_secret_key: InitiatorAddrAndSecretKey,
    ) -> Deploy {
        let serialized_body = serialize_body(&payment, &session);
        let body_hash = Digest::hash(serialized_body);

        let account = match initiator_addr_and_secret_key.initiator_addr() {
            InitiatorAddr::PublicKey(public_key) => public_key,
            InitiatorAddr::AccountHash(_) | InitiatorAddr::EntityAddr(_) => unreachable!(),
        };

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
        let hash = DeployHash::new(Digest::hash(serialized_header));

        let mut deploy = Deploy {
            hash,
            header,
            payment,
            session,
            approvals: BTreeSet::new(),
            #[cfg(any(feature = "once_cell", test))]
            is_valid: OnceCell::new(),
        };

        if let Some(secret_key) = initiator_addr_and_secret_key.secret_key() {
            deploy.sign(secret_key);
        }
        deploy
    }

    /// Returns the `DeployHash` identifying this `Deploy`.
    pub fn hash(&self) -> &DeployHash {
        &self.hash
    }

    /// Returns the public key of the account providing the context in which to run the `Deploy`.
    pub fn account(&self) -> &PublicKey {
        self.header.account()
    }

    /// Returns the creation timestamp of the `Deploy`.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the duration after the creation timestamp for which the `Deploy` will stay valid.
    ///
    /// After this duration has ended, the `Deploy` will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.header.ttl()
    }

    /// Returns `true` if the `Deploy` has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.header.expired(current_instant)
    }

    /// Returns the price per gas unit for the `Deploy`.
    pub fn gas_price(&self) -> u64 {
        self.header.gas_price()
    }

    /// Returns the hash of the body (i.e. the Wasm code) of the `Deploy`.
    pub fn body_hash(&self) -> &Digest {
        self.header.body_hash()
    }

    /// Returns the name of the chain the `Deploy` should be executed on.
    pub fn chain_name(&self) -> &str {
        self.header.chain_name()
    }

    /// Returns a reference to the `DeployHeader` of this `Deploy`.
    pub fn header(&self) -> &DeployHeader {
        &self.header
    }

    /// Consumes `self`, returning the `DeployHeader` of this `Deploy`.
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
    pub fn approvals(&self) -> &BTreeSet<DeployApproval> {
        &self.approvals
    }

    /// Adds a signature of this `Deploy`'s hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let approval = DeployApproval::create(&self.hash, secret_key);
        self.approvals.insert(approval);
    }

    /// Returns the `ApprovalsHash` of this `Deploy`'s approvals.
    pub fn compute_approvals_hash(&self) -> Result<DeployApprovalsHash, bytesrepr::Error> {
        DeployApprovalsHash::compute(&self.approvals)
    }

    /// Returns `true` if the serialized size of the deploy is not greater than
    /// `max_transaction_size`.
    #[cfg(any(feature = "std", test))]
    pub fn is_valid_size(&self, max_transaction_size: u32) -> Result<(), DeployExcessiveSizeError> {
        let deploy_size = self.serialized_length();
        if deploy_size > max_transaction_size as usize {
            return Err(DeployExcessiveSizeError {
                max_transaction_size,
                actual_deploy_size: deploy_size,
            });
        }
        Ok(())
    }

    /// Returns `Ok` if and only if this `Deploy`'s body hashes to the value of `body_hash()`, and
    /// if this `Deploy`'s header hashes to the value claimed as the deploy hash.
    pub fn has_valid_hash(&self) -> Result<(), DeployConfigFailure> {
        let serialized_body = serialize_body(&self.payment, &self.session);
        let body_hash = Digest::hash(serialized_body);
        if body_hash != *self.header.body_hash() {
            #[cfg(any(all(feature = "std", feature = "testing"), test))]
            warn!(?self, ?body_hash, "invalid deploy body hash");
            return Err(DeployConfigFailure::InvalidBodyHash);
        }

        let serialized_header = serialize_header(&self.header);
        let hash = DeployHash::new(Digest::hash(serialized_header));
        if hash != self.hash {
            #[cfg(any(all(feature = "std", feature = "testing"), test))]
            warn!(?self, ?hash, "invalid deploy hash");
            return Err(DeployConfigFailure::InvalidDeployHash);
        }
        Ok(())
    }

    /// Returns `Ok` if and only if:
    ///   * the deploy hash is correct (should be the hash of the header), and
    ///   * the body hash is correct (should be the hash of the body), and
    ///   * approvals are non empty, and
    ///   * all approvals are valid signatures of the deploy hash
    pub fn is_valid(&self) -> Result<(), DeployConfigFailure> {
        #[cfg(any(feature = "once_cell", test))]
        return self.is_valid.get_or_init(|| validate_deploy(self)).clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        validate_deploy(self)
    }

    /// Returns the `DeployFootprint`.
    pub fn footprint(&self) -> Result<DeployFootprint, DeployError> {
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

    /// Returns `Ok` if and only if:
    ///   * the chain_name is correct,
    ///   * the configured parameters are complied with at the given timestamp
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn is_config_compliant(
        &self,
        chain_name: &str,
        config: &TransactionConfig,
        max_associated_keys: u32,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
    ) -> Result<(), DeployConfigFailure> {
        self.is_valid_size(config.max_transaction_size)?;

        let header = self.header();
        if header.chain_name() != chain_name {
            debug!(
                deploy_hash = %self.hash(),
                deploy_header = %header,
                chain_name = %header.chain_name(),
                "invalid chain identifier"
            );
            return Err(DeployConfigFailure::InvalidChainName {
                expected: chain_name.to_string(),
                got: header.chain_name().to_string(),
            });
        }

        header.is_valid(config, timestamp_leeway, at, &self.hash)?;

        if self.approvals.len() > max_associated_keys as usize {
            debug!(
                deploy_hash = %self.hash(),
                number_of_associated_keys = %self.approvals.len(),
                max_associated_keys = %max_associated_keys,
                "number of associated keys exceeds the maximum limit"
            );
            return Err(DeployConfigFailure::ExcessiveApprovals {
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
                .ok_or(DeployConfigFailure::MissingPaymentAmount)?;
            let payment_amount = value
                .clone()
                .into_t::<U512>()
                .map_err(|_| DeployConfigFailure::FailedToParsePaymentAmount)?;
            if payment_amount > U512::from(config.block_gas_limit) {
                debug!(
                    amount = %payment_amount,
                    block_gas_limit = %config.block_gas_limit,
                    "payment amount exceeds block gas limit"
                );
                return Err(DeployConfigFailure::ExceededBlockGasLimit {
                    block_gas_limit: config.block_gas_limit,
                    got: Box::new(payment_amount),
                });
            }
        }

        let payment_args_length = self.payment().args().serialized_length();
        if payment_args_length > config.deploy_config.payment_args_max_length as usize {
            debug!(
                payment_args_length,
                payment_args_max_length = config.deploy_config.payment_args_max_length,
                "payment args excessive"
            );
            return Err(DeployConfigFailure::ExcessivePaymentArgsLength {
                max_length: config.deploy_config.payment_args_max_length as usize,
                got: payment_args_length,
            });
        }

        let session_args_length = self.session().args().serialized_length();
        if session_args_length > config.deploy_config.session_args_max_length as usize {
            debug!(
                session_args_length,
                session_args_max_length = config.deploy_config.session_args_max_length,
                "session args excessive"
            );
            return Err(DeployConfigFailure::ExcessiveSessionArgsLength {
                max_length: config.deploy_config.session_args_max_length as usize,
                got: session_args_length,
            });
        }

        if self.session().is_transfer() {
            let item = self.session().clone();
            let attempted = item
                .args()
                .get(ARG_AMOUNT)
                .ok_or_else(|| {
                    debug!("missing transfer 'amount' runtime argument");
                    DeployConfigFailure::MissingTransferAmount
                })?
                .clone()
                .into_t::<U512>()
                .map_err(|_| {
                    debug!("failed to parse transfer 'amount' runtime argument as a U512");
                    DeployConfigFailure::FailedToParseTransferAmount
                })?;
            let minimum = U512::from(config.native_transfer_minimum_motes);
            if attempted < minimum {
                debug!(
                    minimum = %config.native_transfer_minimum_motes,
                    amount = %attempted,
                    "insufficient transfer amount"
                );
                return Err(DeployConfigFailure::InsufficientTransferAmount {
                    minimum: Box::new(minimum),
                    attempted: Box::new(attempted),
                });
            }
        }

        Ok(())
    }

    // This method is not intended to be used by third party crates.
    //
    // It is required to allow finalized approvals to be injected after reading a `Deploy` from
    // storage.
    #[doc(hidden)]
    pub fn with_approvals(mut self, approvals: BTreeSet<DeployApproval>) -> Self {
        self.approvals = approvals;
        self
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &DEPLOY
    }

    /// Constructs a new signed `Deploy`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
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
        let account_and_secret_key = match account {
            Some(account) => InitiatorAddrAndSecretKey::Both {
                initiator_addr: InitiatorAddr::PublicKey(account),
                secret_key,
            },
            None => InitiatorAddrAndSecretKey::SecretKey(secret_key),
        };

        Deploy::build(
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            payment,
            session,
            account_and_secret_key,
        )
    }

    /// Returns a random `Deploy`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let timestamp = Timestamp::random(rng);
        let ttl = TimeDiff::from_seconds(rng.gen_range(60..300));
        Deploy::random_with_timestamp_and_ttl(rng, timestamp, ttl)
    }

    /// Returns a random `Deploy` but using the specified `timestamp` and `ttl`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
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
            "amount" => U512::from(DEFAULT_MAX_PAYMENT_MOTES),
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

    /// Turns `self` into an invalid `Deploy` by clearing the `chain_name`, invalidating the deploy
    /// hash.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        self.header.invalidate();
    }

    /// Returns a random `Deploy` for a native transfer.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_valid_native_transfer(rng: &mut TestRng) -> Self {
        let timestamp = Timestamp::now();
        let ttl = TimeDiff::from_seconds(rng.gen_range(60..300));
        Self::random_valid_native_transfer_with_timestamp_and_ttl(rng, timestamp, ttl)
    }

    /// Returns a random `Deploy` for a native transfer with timestamp and ttl.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_valid_native_transfer_with_timestamp_and_ttl(
        rng: &mut TestRng,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Self {
        let deploy = Self::random_with_timestamp_and_ttl(rng, timestamp, ttl);
        let transfer_args = runtime_args! {
            "amount" => U512::from(DEFAULT_MIN_TRANSFER_MOTES),
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
            timestamp,
            ttl,
            deploy.header.gas_price(),
            deploy.header.dependencies().clone(),
            deploy.header.chain_name().to_string(),
            payment,
            session,
            &secret_key,
            None,
        )
    }

    /// Returns a random `Deploy` for a native transfer with no dependencies.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_valid_native_transfer_without_deps(rng: &mut TestRng) -> Self {
        let deploy = Self::random(rng);
        let transfer_args = runtime_args! {
            "amount" => U512::from(DEFAULT_MIN_TRANSFER_MOTES),
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

    /// Returns a random invalid `Deploy` without a payment amount specified.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_without_payment_amount(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: RuntimeArgs::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid `Deploy` with an invalid value for the payment amount.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_mangled_payment_amount(rng: &mut TestRng) -> Self {
        let payment_args = runtime_args! {
            "amount" => "invalid-argument"
        };
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: payment_args,
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random `Deploy` with custom payment specified as a stored contract by name.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_valid_custom_payment_contract_by_name(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByName {
            name: "Test".to_string(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid `Deploy` with custom payment specified as a stored contract by
    /// hash, but missing the runtime args.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_missing_payment_contract_by_hash(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid `Deploy` with custom payment specified as a stored contract by
    /// hash, but calling an invalid entry point.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_missing_entry_point_in_payment_contract(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random `Deploy` with custom payment specified as a stored versioned contract by
    /// name.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_valid_custom_payment_package_by_name(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByName {
            name: "Test".to_string(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid `Deploy` with custom payment specified as a stored versioned
    /// contract by hash, but missing the runtime args.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_missing_payment_package_by_hash(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: Default::default(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random invalid `Deploy` with custom payment specified as a stored versioned
    /// contract by hash, but calling an invalid entry point.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_nonexistent_contract_version_in_payment_package(rng: &mut TestRng) -> Self {
        let payment = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: [19; 32].into(),
            version: Some(6u32),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    /// Returns a random `Deploy` with custom session specified as a stored contract by name.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_valid_session_contract_by_name(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByName {
            name: "Test".to_string(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid `Deploy` with custom session specified as a stored contract by
    /// hash, but missing the runtime args.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_missing_session_contract_by_hash(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: Default::default(),
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid `Deploy` with custom session specified as a stored contract by
    /// hash, but calling an invalid entry point.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_missing_entry_point_in_session_contract(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: [19; 32].into(),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random `Deploy` with custom session specified as a stored versioned contract by
    /// name.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_valid_session_package_by_name(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByName {
            name: "Test".to_string(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid `Deploy` with custom session specified as a stored versioned
    /// contract by hash, but missing the runtime args.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_missing_session_package_by_hash(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: Default::default(),
            version: None,
            entry_point: "call".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid `Deploy` with custom session specified as a stored versioned
    /// contract by hash, but calling an invalid entry point.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_nonexistent_contract_version_in_session_package(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::StoredVersionedContractByHash {
            hash: [19; 32].into(),
            version: Some(6u32),
            entry_point: "non-existent-entry-point".to_string(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid transfer `Deploy` with the "target" runtime arg missing.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_without_transfer_target(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "amount" => U512::from(DEFAULT_MIN_TRANSFER_MOTES),
            "source" => PublicKey::random(rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid transfer `Deploy` with the "amount" runtime arg missing.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_without_transfer_amount(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid transfer `Deploy` with an invalid "amount" runtime arg.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_mangled_transfer_amount(rng: &mut TestRng) -> Self {
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

    /// Returns a random invalid `Deploy` with empty session bytes.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_empty_session_module_bytes(rng: &mut TestRng) -> Self {
        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: Default::default(),
        };
        Self::random_transfer_with_session(rng, session)
    }

    /// Returns a random invalid `Deploy` with an expired TTL.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_expired_deploy(rng: &mut TestRng) -> Self {
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

    /// Returns a random `Deploy` with native transfer as payment code.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_native_transfer_in_payment_logic(rng: &mut TestRng) -> Self {
        let transfer_args = runtime_args! {
            "amount" => U512::from(DEFAULT_MIN_TRANSFER_MOTES),
            "source" => PublicKey::random(rng).to_account_hash(),
            "target" => PublicKey::random(rng).to_account_hash(),
        };
        let payment = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Self::random_transfer_with_payment(rng, payment)
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
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

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
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

    /// Creates a withdraw bid deploy, for testing.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn withdraw_bid(
        chain_name: String,
        auction_contract_hash: AddressableEntityHash,
        public_key: PublicKey,
        amount: U512,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! { ARG_AMOUNT => U512::from(3_000_000_000_u64) },
        };
        let args = runtime_args! {
            ARG_AUCTION_AMOUNT => amount,
            ARG_AUCTION_PUBLIC_KEY => public_key.clone(),
        };
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: auction_contract_hash,
            entry_point: METHOD_WITHDRAW_BID.to_string(),
            args,
        };

        Deploy::build(
            timestamp,
            ttl,
            1,
            vec![],
            chain_name,
            payment,
            session,
            InitiatorAddrAndSecretKey::InitiatorAddr(InitiatorAddr::PublicKey(public_key)),
        )
    }

    /// Creates a delegate deploy, for testing.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn delegate(
        chain_name: String,
        auction_contract_hash: AddressableEntityHash,
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        amount: U512,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! { ARG_AMOUNT => U512::from(3_000_000_000_u64) },
        };
        let args = runtime_args! {
            ARG_DELEGATOR => delegator_public_key.clone(),
            ARG_VALIDATOR => validator_public_key,
            ARG_AUCTION_AMOUNT => amount,
        };
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: auction_contract_hash,
            entry_point: METHOD_DELEGATE.to_string(),
            args,
        };

        Deploy::build(
            timestamp,
            ttl,
            1,
            vec![],
            chain_name,
            payment,
            session,
            InitiatorAddrAndSecretKey::InitiatorAddr(InitiatorAddr::PublicKey(
                delegator_public_key,
            )),
        )
    }

    /// Creates an undelegate deploy, for testing.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn undelegate(
        chain_name: String,
        auction_contract_hash: AddressableEntityHash,
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        amount: U512,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! { ARG_AMOUNT => U512::from(3_000_000_000_u64) },
        };
        let args = runtime_args! {
            ARG_DELEGATOR => delegator_public_key.clone(),
            ARG_VALIDATOR => validator_public_key,
            ARG_AUCTION_AMOUNT => amount,
        };
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: auction_contract_hash,
            entry_point: METHOD_UNDELEGATE.to_string(),
            args,
        };

        Deploy::build(
            timestamp,
            ttl,
            1,
            vec![],
            chain_name,
            payment,
            session,
            InitiatorAddrAndSecretKey::InitiatorAddr(InitiatorAddr::PublicKey(
                delegator_public_key,
            )),
        )
    }

    /// Creates an redelegate deploy, for testing.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    #[allow(clippy::too_many_arguments)]
    pub fn redelegate(
        chain_name: String,
        auction_contract_hash: AddressableEntityHash,
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        redelegate_validator_public_key: PublicKey,
        amount: U512,
        timestamp: Timestamp,
        ttl: TimeDiff,
    ) -> Self {
        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! { ARG_AMOUNT => U512::from(3_000_000_000_u64) },
        };
        let args = runtime_args! {
            ARG_DELEGATOR => delegator_public_key.clone(),
            ARG_VALIDATOR => validator_public_key,
            ARG_NEW_VALIDATOR => redelegate_validator_public_key,
            ARG_AUCTION_AMOUNT => amount,
        };
        let session = ExecutableDeployItem::StoredContractByHash {
            hash: auction_contract_hash,
            entry_point: METHOD_REDELEGATE.to_string(),
            args,
        };

        Deploy::build(
            timestamp,
            ttl,
            1,
            vec![],
            chain_name,
            payment,
            session,
            InitiatorAddrAndSecretKey::InitiatorAddr(InitiatorAddr::PublicKey(
                delegator_public_key,
            )),
        )
    }
}

impl hash::Hash for Deploy {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
            is_valid: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
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
        #[cfg(any(feature = "once_cell", test))]
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
            is_valid: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
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
        #[cfg(any(feature = "once_cell", test))]
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
            is_valid: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
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
        let (approvals, remainder) = BTreeSet::<DeployApproval>::from_bytes(remainder)?;
        let maybe_valid_deploy = Deploy {
            header,
            hash,
            payment,
            session,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_valid: OnceCell::new(),
        };
        Ok((maybe_valid_deploy, remainder))
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

fn serialize_header(header: &DeployHeader) -> Vec<u8> {
    header
        .to_bytes()
        .unwrap_or_else(|error| panic!("should serialize deploy header: {}", error))
}

fn serialize_body(payment: &ExecutableDeployItem, session: &ExecutableDeployItem) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(payment.serialized_length() + session.serialized_length());
    payment
        .write_bytes(&mut buffer)
        .unwrap_or_else(|error| panic!("should serialize payment code: {}", error));
    session
        .write_bytes(&mut buffer)
        .unwrap_or_else(|error| panic!("should serialize session code: {}", error));
    buffer
}

/// Computationally expensive validity check for a given deploy instance, including asymmetric_key
/// signing verification.
fn validate_deploy(deploy: &Deploy) -> Result<(), DeployConfigFailure> {
    if deploy.approvals.is_empty() {
        #[cfg(any(all(feature = "std", feature = "testing"), test))]
        warn!(?deploy, "deploy has no approvals");
        return Err(DeployConfigFailure::EmptyApprovals);
    }

    deploy.has_valid_hash()?;

    for (index, approval) in deploy.approvals.iter().enumerate() {
        if let Err(error) = crypto::verify(deploy.hash, approval.signature(), approval.signer()) {
            #[cfg(any(all(feature = "std", feature = "testing"), test))]
            warn!(?deploy, "failed to verify approval {}: {}", index, error);
            return Err(DeployConfigFailure::InvalidApproval { index, error });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{iter, time::Duration};

    use super::*;
    use crate::CLValue;

    const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;

    #[test]
    fn json_roundtrip() {
        let mut rng = TestRng::new();
        let deploy = Deploy::random(&mut rng);
        let json_string = serde_json::to_string_pretty(&deploy).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(deploy, decoded);
    }

    #[test]
    fn bincode_roundtrip() {
        let mut rng = TestRng::new();
        let deploy = Deploy::random(&mut rng);
        let serialized = bincode::serialize(&deploy).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(deploy, deserialized);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
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
            let value = CLValue::from_t(U512::from(DEFAULT_MIN_TRANSFER_MOTES))
                .expect("should create CLValue");
            transfer_args.insert_cl_value("amount", value);
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
        let mut rng = TestRng::new();
        let deploy = create_deploy(&mut rng, TransactionConfig::default().max_ttl, 0, "net-1");
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

    fn check_is_not_valid(invalid_deploy: Deploy, expected_error: DeployConfigFailure) {
        assert!(
            invalid_deploy.is_valid.get().is_none(),
            "is valid should initially be None"
        );
        let actual_error = invalid_deploy.is_valid().unwrap_err();

        // Ignore the `error_msg` field of `InvalidApproval` when comparing to expected error, as
        // this makes the test too fragile.  Otherwise expect the actual error should exactly match
        // the expected error.
        match expected_error {
            DeployConfigFailure::InvalidApproval {
                index: expected_index,
                ..
            } => match actual_error {
                DeployConfigFailure::InvalidApproval {
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
        let mut rng = TestRng::new();
        let mut deploy = create_deploy(&mut rng, TransactionConfig::default().max_ttl, 0, "net-1");

        deploy.session = ExecutableDeployItem::Transfer {
            args: runtime_args! {
                "amount" => 1
            },
        };
        check_is_not_valid(deploy, DeployConfigFailure::InvalidBodyHash);
    }

    #[test]
    fn not_valid_due_to_invalid_deploy_hash() {
        let mut rng = TestRng::new();
        let mut deploy = create_deploy(&mut rng, TransactionConfig::default().max_ttl, 0, "net-1");

        // deploy.header.gas_price = 2;
        deploy.invalidate();
        check_is_not_valid(deploy, DeployConfigFailure::InvalidDeployHash);
    }

    #[test]
    fn not_valid_due_to_empty_approvals() {
        let mut rng = TestRng::new();
        let mut deploy = create_deploy(&mut rng, TransactionConfig::default().max_ttl, 0, "net-1");
        deploy.approvals = BTreeSet::new();
        assert!(deploy.approvals.is_empty());
        check_is_not_valid(deploy, DeployConfigFailure::EmptyApprovals)
    }

    #[test]
    fn not_valid_due_to_invalid_approval() {
        let mut rng = TestRng::new();
        let mut deploy = create_deploy(&mut rng, TransactionConfig::default().max_ttl, 0, "net-1");

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
            DeployConfigFailure::InvalidApproval {
                index: expected_index,
                error: crypto::Error::SignatureError, // This field is ignored in the check.
            },
        );
    }

    #[test]
    fn is_acceptable() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();

        let deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );
        let current_timestamp = deploy.header().timestamp();
        deploy
            .is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp,
            )
            .expect("should be acceptable");
    }

    #[test]
    fn not_acceptable_due_to_invalid_chain_name() {
        let mut rng = TestRng::new();
        let expected_chain_name = "net-1";
        let wrong_chain_name = "net-2".to_string();
        let config = TransactionConfig::default();

        let deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            &wrong_chain_name,
        );

        let expected_error = DeployConfigFailure::InvalidChainName {
            expected: expected_chain_name.to_string(),
            got: wrong_chain_name,
        };

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            deploy.is_config_compliant(
                expected_chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
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
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();

        let dependency_count = usize::from(config.deploy_config.max_dependencies + 1);

        let deploy = create_deploy(&mut rng, config.max_ttl, dependency_count, chain_name);

        let expected_error = DeployConfigFailure::ExcessiveDependencies {
            max_dependencies: config.deploy_config.max_dependencies,
            got: dependency_count,
        };

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_ttl() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();

        let ttl = config.max_ttl + TimeDiff::from(Duration::from_secs(1));

        let deploy = create_deploy(
            &mut rng,
            ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );

        let expected_error = DeployConfigFailure::ExcessiveTimeToLive {
            max_ttl: config.max_ttl,
            got: ttl,
        };

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_timestamp_in_future() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let leeway = TimeDiff::from_seconds(2);

        let deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );
        let current_timestamp = deploy.header.timestamp() - leeway - TimeDiff::from_seconds(1);

        let expected_error = DeployConfigFailure::TimestampInFuture {
            validation_timestamp: current_timestamp,
            timestamp_leeway: leeway,
            got: deploy.header.timestamp(),
        };

        assert_eq!(
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                leeway,
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn acceptable_if_timestamp_slightly_in_future() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let leeway = TimeDiff::from_seconds(2);

        let deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );
        let current_timestamp = deploy.header.timestamp() - (leeway / 2);
        deploy
            .is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                leeway,
                current_timestamp,
            )
            .expect("should be acceptable");
    }

    #[test]
    fn not_acceptable_due_to_missing_payment_amount() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();

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
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );

        deploy.payment = payment;
        deploy.session = session;

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            ),
            Err(DeployConfigFailure::MissingPaymentAmount)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_mangled_payment_amount() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();

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
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );

        deploy.payment = payment;
        deploy.session = session;

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            ),
            Err(DeployConfigFailure::FailedToParsePaymentAmount)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_payment_amount() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let amount = U512::from(config.block_gas_limit + 1);

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
            config.max_ttl,
            config.deploy_config.max_dependencies.into(),
            chain_name,
        );

        deploy.payment = payment;
        deploy.session = session;

        let expected_error = DeployConfigFailure::ExceededBlockGasLimit {
            block_gas_limit: config.block_gas_limit,
            got: Box::new(amount),
        };

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            deploy.is_valid.get().is_none(),
            "deploy should not have run expensive `is_valid` call"
        );
    }

    #[test]
    fn transfer_acceptable_regardless_of_excessive_payment_amount() {
        let mut rng = TestRng::new();
        let secret_key = SecretKey::random(&mut rng);
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let amount = U512::from(config.block_gas_limit + 1);

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! {
                "amount" => amount
            },
        };

        let transfer_args = {
            let mut transfer_args = RuntimeArgs::new();
            let value = CLValue::from_t(U512::from(DEFAULT_MIN_TRANSFER_MOTES))
                .expect("should create CLValue");
            transfer_args.insert_cl_value("amount", value);
            transfer_args
        };

        let deploy = Deploy::new(
            Timestamp::now(),
            config.max_ttl,
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

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            Ok(()),
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            )
        )
    }

    #[test]
    fn not_acceptable_due_to_excessive_approvals() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies as usize,
            chain_name,
        );
        // This test is to ensure a given limit is being checked.
        // Therefore, set the limit to one less than the approvals in the deploy.
        let max_associated_keys = (deploy.approvals.len() - 1) as u32;
        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            Err(DeployConfigFailure::ExcessiveApprovals {
                got: deploy.approvals.len() as u32,
                max_associated_keys: (deploy.approvals.len() - 1) as u32
            }),
            deploy.is_config_compliant(
                chain_name,
                &config,
                max_associated_keys,
                TimeDiff::default(),
                current_timestamp
            )
        )
    }

    #[test]
    fn not_acceptable_due_to_missing_transfer_amount() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let mut deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies as usize,
            chain_name,
        );

        let transfer_args = RuntimeArgs::default();
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        deploy.session = session;

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            Err(DeployConfigFailure::MissingTransferAmount),
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            )
        )
    }

    #[test]
    fn not_acceptable_due_to_mangled_transfer_amount() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let mut deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies as usize,
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

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            Err(DeployConfigFailure::FailedToParseTransferAmount),
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            )
        )
    }

    #[test]
    fn not_acceptable_due_to_insufficient_transfer_amount() {
        let mut rng = TestRng::new();
        let chain_name = "net-1";
        let config = TransactionConfig::default();
        let mut deploy = create_deploy(
            &mut rng,
            config.max_ttl,
            config.deploy_config.max_dependencies as usize,
            chain_name,
        );

        let amount = config.native_transfer_minimum_motes - 1;
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

        let current_timestamp = deploy.header().timestamp();
        assert_eq!(
            Err(DeployConfigFailure::InsufficientTransferAmount {
                minimum: Box::new(U512::from(config.native_transfer_minimum_motes)),
                attempted: Box::new(insufficient_amount),
            }),
            deploy.is_config_compliant(
                chain_name,
                &config,
                DEFAULT_MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
                current_timestamp
            )
        )
    }
}
