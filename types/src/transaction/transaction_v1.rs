mod errors_v1;
mod transaction_v1_body;
#[cfg(any(feature = "std", test))]
mod transaction_v1_builder;
mod transaction_v1_category;
mod transaction_v1_hash;
mod transaction_v1_header;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use alloc::string::ToString;
use alloc::{collections::BTreeSet, vec::Vec};
use core::{
    cmp,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::{
    Approval, ApprovalsHash, InitiatorAddr, PricingMode, TransactionEntryPoint, TransactionRuntime,
    TransactionScheduling, TransactionTarget,
};
#[cfg(any(feature = "std", test))]
use super::{GasLimited, InitiatorAddrAndSecretKey};
#[cfg(any(feature = "std", test))]
use crate::chainspec::Chainspec;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::chainspec::PricingHandling;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, Digest, DisplayIter, SecretKey, TimeDiff, Timestamp,
};

#[cfg(any(feature = "std", test))]
use crate::{Gas, Motes, TransactionConfig, U512};
pub use errors_v1::{
    DecodeFromJsonErrorV1 as TransactionV1DecodeFromJsonError, ErrorV1 as TransactionV1Error,
    ExcessiveSizeErrorV1 as TransactionV1ExcessiveSizeError,
    InvalidTransaction as InvalidTransactionV1,
};
pub use transaction_v1_body::{TransactionArgs, TransactionV1Body};
#[cfg(any(feature = "std", test))]
pub use transaction_v1_builder::{TransactionV1Builder, TransactionV1BuilderError};
pub use transaction_v1_category::TransactionCategory;
pub use transaction_v1_hash::TransactionV1Hash;
pub use transaction_v1_header::TransactionV1Header;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;

/// A unit of work sent by a client to the network, which when executed can cause global state to
/// be altered.
///
/// To construct a new `TransactionV1`, use a [`TransactionV1Builder`].
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
    schemars(
        description = "A unit of work sent by a client to the network, which when executed can \
        cause global state to be altered."
    )
)]
pub struct TransactionV1 {
    hash: TransactionV1Hash,
    header: TransactionV1Header,
    body: TransactionV1Body,
    approvals: BTreeSet<Approval>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    is_verified: OnceCell<Result<(), InvalidTransactionV1>>,
}

impl TransactionV1 {
    /// Called by the `TransactionV1Builder` to construct a new `TransactionV1`.
    #[cfg(any(feature = "std", test))]
    pub(super) fn build(
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        body: TransactionV1Body,
        pricing_mode: PricingMode,
        initiator_addr_and_secret_key: InitiatorAddrAndSecretKey,
    ) -> TransactionV1 {
        let initiator_addr = initiator_addr_and_secret_key.initiator_addr();
        let body_hash = Digest::hash(
            body.to_bytes()
                .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
        );
        let header = TransactionV1Header::new(
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            initiator_addr,
        );

        let hash = header.compute_hash();
        let mut transaction = TransactionV1 {
            hash,
            header,
            body,
            approvals: BTreeSet::new(),
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };

        if let Some(secret_key) = initiator_addr_and_secret_key.secret_key() {
            transaction.sign(secret_key);
        }
        transaction
    }

    /// Returns the hash identifying this transaction.
    pub fn hash(&self) -> &TransactionV1Hash {
        &self.hash
    }

    /// Returns the name of the chain the transaction should be executed on.
    pub fn chain_name(&self) -> &str {
        self.header.chain_name()
    }

    /// Returns the creation timestamp of the transaction.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the duration after the creation timestamp for which the transaction will stay valid.
    ///
    /// After this duration has ended, the transaction will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.header.ttl()
    }

    /// Returns `true` if the transaction has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.header.expired(current_instant)
    }

    /// Returns the pricing mode for the transaction.
    pub fn pricing_mode(&self) -> &PricingMode {
        self.header.pricing_mode()
    }

    /// Returns the address of the initiator of the transaction.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        self.header.initiator_addr()
    }

    /// Returns a reference to the header of this transaction.
    pub fn header(&self) -> &TransactionV1Header {
        &self.header
    }

    /// Consumes `self`, returning the header of this transaction.
    pub fn take_header(self) -> TransactionV1Header {
        self.header
    }

    /// Returns the runtime args of the transaction.
    pub fn args(&self) -> &TransactionArgs {
        self.body.args()
    }

    /// Consumes `self`, returning the runtime args of the transaction.
    pub fn take_args(self) -> TransactionArgs {
        self.body.take_args()
    }

    /// Returns the target of the transaction.
    pub fn target(&self) -> &TransactionTarget {
        self.body.target()
    }

    /// Returns the entry point of the transaction.
    pub fn entry_point(&self) -> &TransactionEntryPoint {
        self.body.entry_point()
    }

    /// Returns the scheduling kind of the transaction.
    pub fn scheduling(&self) -> &TransactionScheduling {
        self.body.scheduling()
    }

    /// Returns the body of this transaction.
    pub fn body(&self) -> &TransactionV1Body {
        &self.body
    }

    /// Returns true if this transaction is a native mint interaction.
    pub fn is_native_mint(&self) -> bool {
        self.body().is_native_mint()
    }

    /// Returns true if this transaction is a native auction interaction.
    pub fn is_native_auction(&self) -> bool {
        self.body().is_native_auction()
    }

    /// Returns true if this transaction is a smart contract installer or upgrader.
    pub fn is_install_or_upgrade(&self) -> bool {
        self.body().is_install_or_upgrade()
    }

    /// Returns the transaction category.
    pub fn transaction_category(&self) -> u8 {
        self.body.transaction_category()
    }

    /// Does this transaction have wasm targeting the v1 vm.
    pub fn is_v1_wasm(&self) -> bool {
        match self.target() {
            TransactionTarget::Native => false,
            TransactionTarget::Stored { runtime, .. }
            | TransactionTarget::Session { runtime, .. } => {
                matches!(runtime, TransactionRuntime::VmCasperV1)
                    && (!self.is_native_mint() && !self.is_native_auction())
            }
        }
    }

    /// Does this transaction have wasm targeting the v2 vm.
    pub fn is_v2_wasm(&self) -> bool {
        match self.target() {
            TransactionTarget::Native => false,
            TransactionTarget::Stored { runtime, .. }
            | TransactionTarget::Session { runtime, .. } => {
                matches!(runtime, TransactionRuntime::VmCasperV2)
                    && (!self.is_native_mint() && !self.is_native_auction())
            }
        }
    }

    /// Should this transaction start in the initiating accounts context?
    pub fn is_account_session(&self) -> bool {
        let target_is_stored_contract = matches!(self.target(), TransactionTarget::Stored { .. });
        !target_is_stored_contract
    }

    /// Returns the approvals for this transaction.
    pub fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }

    /// Consumes `self`, returning a tuple of its constituent parts.
    pub fn destructure(
        self,
    ) -> (
        TransactionV1Hash,
        TransactionV1Header,
        TransactionV1Body,
        BTreeSet<Approval>,
    ) {
        (self.hash, self.header, self.body, self.approvals)
    }

    /// Adds a signature of this transaction's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let approval = Approval::create(&self.hash.into(), secret_key);
        self.approvals.insert(approval);
    }

    /// Returns the `ApprovalsHash` of this transaction's approvals.
    pub fn compute_approvals_hash(&self) -> Result<ApprovalsHash, bytesrepr::Error> {
        ApprovalsHash::compute(&self.approvals)
    }

    /// Returns `true` if the serialized size of the transaction is not greater than
    /// `max_transaction_size`.
    #[cfg(any(feature = "std", test))]
    fn is_valid_size(
        &self,
        max_transaction_size: u32,
    ) -> Result<(), TransactionV1ExcessiveSizeError> {
        let actual_transaction_size = self.serialized_length();
        if actual_transaction_size > max_transaction_size as usize {
            return Err(TransactionV1ExcessiveSizeError {
                max_transaction_size,
                actual_transaction_size,
            });
        }
        Ok(())
    }

    /// Returns `Ok` if and only if this transaction's body hashes to the value of `body_hash()`,
    /// and if this transaction's header hashes to the value claimed as the transaction hash.
    pub fn has_valid_hash(&self) -> Result<(), InvalidTransactionV1> {
        let body_hash = Digest::hash(
            self.body
                .to_bytes()
                .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
        );
        if body_hash != *self.header.body_hash() {
            debug!(?self, ?body_hash, "invalid transaction body hash");
            return Err(InvalidTransactionV1::InvalidBodyHash);
        }

        let hash = TransactionV1Hash::new(Digest::hash(
            self.header
                .to_bytes()
                .unwrap_or_else(|error| panic!("should serialize header: {}", error)),
        ));
        if hash != self.hash {
            debug!(?self, ?hash, "invalid transaction hash");
            return Err(InvalidTransactionV1::InvalidTransactionHash);
        }
        Ok(())
    }

    /// Returns `Ok` if and only if:
    ///   * the transaction hash is correct (see [`TransactionV1::has_valid_hash`] for details)
    ///   * approvals are non empty, and
    ///   * all approvals are valid signatures of the signed hash
    pub fn verify(&self) -> Result<(), InvalidTransactionV1> {
        #[cfg(any(feature = "once_cell", test))]
        return self.is_verified.get_or_init(|| self.do_verify()).clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        self.do_verify()
    }

    fn do_verify(&self) -> Result<(), InvalidTransactionV1> {
        if self.approvals.is_empty() {
            debug!(?self, "transaction has no approvals");
            return Err(InvalidTransactionV1::EmptyApprovals);
        }

        self.has_valid_hash()?;

        for (index, approval) in self.approvals.iter().enumerate() {
            if let Err(error) = crypto::verify(self.hash, approval.signature(), approval.signer()) {
                debug!(
                    ?self,
                    "failed to verify transaction approval {}: {}", index, error
                );
                return Err(InvalidTransactionV1::InvalidApproval { index, error });
            }
        }

        Ok(())
    }

    /// Returns `Ok` if and only if:
    ///   * the chain_name is correct,
    ///   * the configured parameters are complied with at the given timestamp
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn is_config_compliant(
        &self,
        chainspec: &Chainspec,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
    ) -> Result<(), InvalidTransactionV1> {
        let transaction_config = chainspec.transaction_config.clone();
        self.is_valid_size(
            transaction_config
                .transaction_v1_config
                .get_max_serialized_length(self.body.transaction_category) as u32,
        )?;

        let chain_name = chainspec.network_config.name.clone();

        let header = self.header();
        if header.chain_name() != chain_name {
            debug!(
                transaction_hash = %self.hash(),
                transaction_header = %header,
                chain_name = %header.chain_name(),
                "invalid chain identifier"
            );
            return Err(InvalidTransactionV1::InvalidChainName {
                expected: chain_name,
                got: header.chain_name().to_string(),
            });
        }

        let price_handling = chainspec.core_config.pricing_handling;
        let price_mode = header.pricing_mode();

        match price_mode {
            PricingMode::Classic { .. } => {
                if let PricingHandling::Classic = price_handling {
                } else {
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: price_mode.clone(),
                    });
                }
            }
            PricingMode::Fixed { .. } => {
                if let PricingHandling::Fixed = price_handling {
                } else {
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: price_mode.clone(),
                    });
                }
            }
            PricingMode::Reserved { .. } => {
                if !chainspec.core_config.allow_reservations {
                    // Currently Reserved isn't implemented and we should
                    // not be accepting transactions with this mode.
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: price_mode.clone(),
                    });
                }
            }
            PricingMode::GasLimited { .. } => {
                if let PricingHandling::GasLimited = price_handling {
                } else {
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: price_mode.clone(),
                    });
                }
            }
        }

        header.is_valid(&transaction_config, timestamp_leeway, at, &self.hash)?;

        let max_associated_keys = chainspec.core_config.max_associated_keys;

        if self.approvals.len() > max_associated_keys as usize {
            debug!(
                transaction_hash = %self.hash(),
                number_of_approvals = %self.approvals.len(),
                max_associated_keys = %max_associated_keys,
                "number of transaction approvals exceeds the limit"
            );
            return Err(InvalidTransactionV1::ExcessiveApprovals {
                got: self.approvals.len() as u32,
                max_associated_keys,
            });
        }

        let gas_limit = self.gas_limit(chainspec)?;
        let block_gas_limit = Gas::new(U512::from(transaction_config.block_gas_limit));
        if gas_limit > block_gas_limit {
            debug!(
                amount = %gas_limit,
                %block_gas_limit,
                "transaction gas limit exceeds block gas limit"
            );
            return Err(InvalidTransactionV1::ExceedsBlockGasLimit {
                block_gas_limit: transaction_config.block_gas_limit,
                got: Box::new(gas_limit.value()),
            });
        }

        self.body.is_valid(&transaction_config)
    }

    // This method is not intended to be used by third party crates.
    //
    // It is required to allow finalized approvals to be injected after reading a transaction from
    // storage.
    #[doc(hidden)]
    pub fn with_approvals(mut self, approvals: BTreeSet<Approval>) -> Self {
        self.approvals = approvals;
        self
    }

    /// Returns a random, valid but possibly expired transaction.
    ///
    /// Note that the [`TransactionV1Builder`] can be used to create a random transaction with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransactionV1Builder::new_random(rng).build().unwrap()
    }

    /// Returns a random transaction with "transfer" category.
    ///
    /// Note that the [`TransactionV1Builder`] can be used to create a random transaction with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_transfer(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        let transaction = TransactionV1Builder::new_random_with_category_and_timestamp_and_ttl(
            rng,
            TransactionCategory::Mint as u8,
            timestamp,
            ttl,
        )
        .build()
        .unwrap();
        assert_eq!(
            transaction.transaction_category(),
            TransactionCategory::Mint as u8,
            "Required mint, incorrect category"
        );
        transaction
    }

    /// Returns a random transaction with "standard" category.
    ///
    /// Note that the [`TransactionV1Builder`] can be used to create a random transaction with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_wasm(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        let transaction = TransactionV1Builder::new_random_with_category_and_timestamp_and_ttl(
            rng,
            TransactionCategory::Large as u8,
            timestamp,
            ttl,
        )
        .build()
        .unwrap();
        assert_eq!(
            transaction.transaction_category(),
            TransactionCategory::Large as u8,
            "Required large, incorrect category"
        );
        transaction
    }

    /// Returns a random transaction with "install/upgrade" category.
    ///
    /// Note that the [`TransactionV1Builder`] can be used to create a random transaction with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_install_upgrade(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        let transaction = TransactionV1Builder::new_random_with_category_and_timestamp_and_ttl(
            rng,
            TransactionCategory::InstallUpgrade as u8,
            timestamp,
            ttl,
        )
        .build()
        .unwrap();
        assert_eq!(
            transaction.transaction_category(),
            TransactionCategory::InstallUpgrade as u8,
            "Required install/upgrade, incorrect category"
        );
        transaction
    }

    /// Returns a random transaction with "install/upgrade" category.
    ///
    /// Note that the [`TransactionV1Builder`] can be used to create a random transaction with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_auction(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        let transaction = TransactionV1Builder::new_random_with_category_and_timestamp_and_ttl(
            rng,
            TransactionCategory::Auction as u8,
            timestamp,
            ttl,
        )
        .build()
        .unwrap();
        assert_eq!(
            transaction.transaction_category(),
            TransactionCategory::Auction as u8,
            "Required auction, incorrect category"
        );
        transaction
    }

    /// Turns `self` into an invalid transaction by clearing the `chain_name`, invalidating the
    /// transaction header hash.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        self.header.invalidate();
    }

    /// Used by the `TestTransactionV1Builder` to inject invalid approvals for testing purposes.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn apply_approvals(&mut self, approvals: Vec<Approval>) {
        self.approvals.extend(approvals);
    }
}

#[cfg(any(feature = "std", test))]
impl GasLimited for TransactionV1 {
    type Error = InvalidTransactionV1;

    fn gas_cost(&self, chainspec: &Chainspec, gas_price: u8) -> Result<Motes, Self::Error> {
        let gas_limit = self.gas_limit(chainspec)?;
        let motes = match self.header().pricing_mode() {
            PricingMode::Classic { .. } | PricingMode::Fixed { .. } => {
                Motes::from_gas(gas_limit, gas_price)
                    .ok_or(InvalidTransactionV1::UnableToCalculateGasCost)?
            }
            PricingMode::Reserved { .. } => {
                Motes::zero() // prepaid
            }
            PricingMode::GasLimited {
                gas_limit: _,
                gas_price_tolerance,
            } => {
                let gas_price = Gas::new(U512::from(gas_price));
                let gas = Gas::new(U512::from(gas_limit.value() * gas_price.value()));
                Motes::from_gas(gas, *gas_price_tolerance)
                    .ok_or(InvalidTransactionV1::UnableToCalculateGasCost)?
            }
        };
        Ok(motes)
    }

    fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, Self::Error> {
        let costs = chainspec.system_costs_config;
        let gas = match self.header().pricing_mode() {
            PricingMode::Classic { payment_amount, .. } => Gas::new(*payment_amount),
            PricingMode::Fixed { .. } => {
                let computation_limit = {
                    if self.is_native_mint() {
                        // Because we currently only support one native mint interaction,
                        // native transfer, we can short circuit to return that value.
                        // However if other direct mint interactions are supported
                        // in the future (such as the upcoming burn feature),
                        // this logic will need to be expanded to self.mint_costs().field?
                        // for the value for each verb...see how auction is set up below.
                        costs.mint_costs().transfer as u64
                    } else if self.is_native_auction() {
                        let entry_point = self.body().entry_point();
                        let amount = match entry_point {
                            TransactionEntryPoint::Call => {
                                return Err(InvalidTransactionV1::EntryPointCannotBeCall)
                            }
                            TransactionEntryPoint::Custom(_) | TransactionEntryPoint::Transfer => {
                                return Err(InvalidTransactionV1::EntryPointCannotBeCustom {
                                    entry_point: entry_point.clone(),
                                });
                            }
                            TransactionEntryPoint::AddBid | TransactionEntryPoint::ActivateBid => {
                                costs.auction_costs().add_bid.into()
                            }
                            TransactionEntryPoint::WithdrawBid => {
                                costs.auction_costs().withdraw_bid.into()
                            }
                            TransactionEntryPoint::Delegate => {
                                costs.auction_costs().delegate.into()
                            }
                            TransactionEntryPoint::Undelegate => {
                                costs.auction_costs().undelegate.into()
                            }
                            TransactionEntryPoint::Redelegate => {
                                costs.auction_costs().redelegate.into()
                            }
                            TransactionEntryPoint::ChangeBidPublicKey => {
                                costs.auction_costs().change_bid_public_key
                            }
                            TransactionEntryPoint::DefaultInstantiate => {
                                return Err(InvalidTransactionV1::UnableToInstantiate)
                            }
                            TransactionEntryPoint::Instantiate(_) => {
                                return Err(InvalidTransactionV1::UnableToInstantiate)
                            }
                        };
                        amount
                    } else {
                        chainspec.get_max_gas_limit_by_category(self.body.transaction_category)
                    }
                };
                Gas::new(U512::from(computation_limit))
            }
            PricingMode::Reserved { receipt } => {
                return Err(InvalidTransactionV1::InvalidPricingMode {
                    price_mode: PricingMode::Reserved { receipt: *receipt },
                });
            }
            PricingMode::GasLimited {
                gas_limit,
                gas_price_tolerance,
            } => {
                let gas_limit = Gas::new(U512::from(*gas_limit));
                let gas_price = Gas::new(U512::from(*gas_price_tolerance));
                let gas = Gas::new(U512::from(gas_limit.value() * gas_price.value()));
                gas
            }
        };
        Ok(gas)
    }

    fn gas_price_tolerance(&self) -> Result<u8, Self::Error> {
        Ok(self.header.gas_price_tolerance())
    }
}

impl hash::Hash for TransactionV1 {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        hash.hash(state);
        header.hash(state);
        body.hash(state);
        approvals.hash(state);
    }
}

impl PartialEq for TransactionV1 {
    fn eq(&self, other: &TransactionV1) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        *hash == other.hash
            && *header == other.header
            && *body == other.body
            && *approvals == other.approvals
    }
}

impl Ord for TransactionV1 {
    fn cmp(&self, other: &TransactionV1) -> cmp::Ordering {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        hash.cmp(&other.hash)
            .then_with(|| header.cmp(&other.header))
            .then_with(|| body.cmp(&other.body))
            .then_with(|| approvals.cmp(&other.approvals))
    }
}

impl PartialOrd for TransactionV1 {
    fn partial_cmp(&self, other: &TransactionV1) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ToBytes for TransactionV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.hash.write_bytes(writer)?;
        self.header.write_bytes(writer)?;
        self.body.write_bytes(writer)?;
        self.approvals.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.header.serialized_length()
            + self.body.serialized_length()
            + self.approvals.serialized_length()
    }
}

impl FromBytes for TransactionV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = TransactionV1Hash::from_bytes(bytes)?;
        let (header, remainder) = TransactionV1Header::from_bytes(remainder)?;
        let (body, remainder) = TransactionV1Body::from_bytes(remainder)?;
        let (approvals, remainder) = BTreeSet::<Approval>::from_bytes(remainder)?;
        let transaction = TransactionV1 {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };
        Ok((transaction, remainder))
    }
}

impl Display for TransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-v1[{}, {}, approvals: {}]",
            self.header,
            self.body,
            DisplayIter::new(self.approvals.iter())
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const MAX_ASSOCIATED_KEYS: u32 = 5;

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        let json_string = serde_json::to_string_pretty(&transaction).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(transaction, decoded);
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        let serialized = bincode::serialize(&transaction).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(transaction, deserialized);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        bytesrepr::test_serialization_roundtrip(transaction.header());
        bytesrepr::test_serialization_roundtrip(&transaction);
    }

    #[test]
    fn is_valid() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        assert_eq!(
            transaction.is_verified.get(),
            None,
            "is_verified should initially be None"
        );
        transaction.verify().expect("should verify");
        assert_eq!(
            transaction.is_verified.get(),
            Some(&Ok(())),
            "is_verified should be true"
        );
    }

    fn check_is_not_valid(
        invalid_transaction: TransactionV1,
        expected_error: InvalidTransactionV1,
    ) {
        assert!(
            invalid_transaction.is_verified.get().is_none(),
            "is_verified should initially be None"
        );
        let actual_error = invalid_transaction.verify().unwrap_err();

        // Ignore the `error_msg` field of `InvalidApproval` when comparing to expected error, as
        // this makes the test too fragile.  Otherwise expect the actual error should exactly match
        // the expected error.
        match expected_error {
            InvalidTransactionV1::InvalidApproval {
                index: expected_index,
                ..
            } => match actual_error {
                InvalidTransactionV1::InvalidApproval {
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
            invalid_transaction.is_verified.get(),
            Some(&Err(actual_error)),
            "is_verified should now be Some"
        );
    }

    #[test]
    fn not_valid_due_to_invalid_transaction_hash() {
        let rng = &mut TestRng::new();
        let mut transaction = TransactionV1::random(rng);

        transaction.invalidate();
        check_is_not_valid(transaction, InvalidTransactionV1::InvalidTransactionHash);
    }

    #[test]
    fn not_valid_due_to_empty_approvals() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1Builder::new_random(rng)
            .with_no_secret_key()
            .build()
            .unwrap();
        assert!(transaction.approvals.is_empty());
        check_is_not_valid(transaction, InvalidTransactionV1::EmptyApprovals)
    }

    #[test]
    fn not_valid_due_to_invalid_approval() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1Builder::new_random(rng)
            .with_invalid_approval(rng)
            .build()
            .unwrap();

        // The expected index for the invalid approval will be the first index at which there is an
        // approval where the signer is not the account holder.
        let account_holder = match transaction.initiator_addr() {
            InitiatorAddr::PublicKey(public_key) => public_key.clone(),
            InitiatorAddr::AccountHash(_) => unreachable!(),
        };
        let expected_index = transaction
            .approvals
            .iter()
            .enumerate()
            .find(|(_, approval)| approval.signer() != &account_holder)
            .map(|(index, _)| index)
            .unwrap();
        check_is_not_valid(
            transaction,
            InvalidTransactionV1::InvalidApproval {
                index: expected_index,
                error: crypto::Error::SignatureError, // This field is ignored in the check.
            },
        );
    }

    #[test]
    fn is_config_compliant() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction = TransactionV1Builder::new_random_with_category_and_timestamp_and_ttl(
            rng,
            TransactionCategory::Large as u8,
            None,
            None,
        )
        .with_chain_name(chain_name)
        .build()
        .unwrap();
        let current_timestamp = transaction.timestamp();
        let chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret
        };
        transaction
            .is_config_compliant(&chainspec, TimeDiff::default(), current_timestamp)
            .expect("should be acceptable");
    }

    #[test]
    fn not_acceptable_due_to_invalid_chain_name() {
        let rng = &mut TestRng::new();
        let expected_chain_name = "net-1";
        let wrong_chain_name = "net-2";
        let transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(wrong_chain_name)
            .build()
            .unwrap();

        let expected_error = InvalidTransactionV1::InvalidChainName {
            expected: expected_chain_name.to_string(),
            got: wrong_chain_name.to_string(),
        };

        let current_timestamp = transaction.timestamp();
        let chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = expected_chain_name.to_string();
            ret
        };
        assert_eq!(
            transaction.is_config_compliant(&chainspec, TimeDiff::default(), current_timestamp),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_ttl() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction_config = TransactionConfig::default();
        let ttl = transaction_config.max_ttl + TimeDiff::from(Duration::from_secs(1));
        let transaction = TransactionV1Builder::new_random(rng)
            .with_ttl(ttl)
            .with_chain_name(chain_name)
            .build()
            .unwrap();

        let expected_error = InvalidTransactionV1::ExcessiveTimeToLive {
            max_ttl: transaction_config.max_ttl,
            got: ttl,
        };

        let current_timestamp = transaction.timestamp();
        let chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret
        };
        assert_eq!(
            transaction.is_config_compliant(&chainspec, TimeDiff::default(), current_timestamp),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_timestamp_in_future() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let leeway = TimeDiff::from_seconds(2);

        let transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .build()
            .unwrap();
        let current_timestamp = transaction.timestamp() - leeway - TimeDiff::from_seconds(1);

        let expected_error = InvalidTransactionV1::TimestampInFuture {
            validation_timestamp: current_timestamp,
            timestamp_leeway: leeway,
            got: transaction.timestamp(),
        };

        let chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret
        };

        assert_eq!(
            transaction.is_config_compliant(&chainspec, leeway, current_timestamp),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_approvals() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let mut transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .build()
            .unwrap();

        for _ in 0..MAX_ASSOCIATED_KEYS {
            transaction.sign(&SecretKey::random(rng));
        }

        let current_timestamp = transaction.timestamp();

        let expected_error = InvalidTransactionV1::ExcessiveApprovals {
            got: MAX_ASSOCIATED_KEYS + 1,
            max_associated_keys: MAX_ASSOCIATED_KEYS,
        };

        let chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret.core_config.max_associated_keys = MAX_ASSOCIATED_KEYS;
            ret
        };

        assert_eq!(
            transaction.is_config_compliant(&chainspec, TimeDiff::default(), current_timestamp),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_invalid_pricing_modes() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";

        let reserved_mode = PricingMode::Reserved {
            receipt: Default::default(),
        };

        let reserved_transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .with_pricing_mode(reserved_mode.clone())
            .build()
            .expect("must be able to create a reserved transaction");

        let chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret
        };

        let current_timestamp = reserved_transaction.timestamp();
        let expected_error = InvalidTransactionV1::InvalidPricingMode {
            price_mode: reserved_mode,
        };
        assert_eq!(
            reserved_transaction.is_config_compliant(
                &chainspec,
                TimeDiff::default(),
                current_timestamp,
            ),
            Err(expected_error)
        );
        assert!(
            reserved_transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );

        let fixed_mode_transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: 1u8,
            })
            .build()
            .expect("must create fixed mode transaction");

        let fixed_handling_chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret.core_config.pricing_handling = PricingHandling::Fixed;
            ret
        };

        let classic_handling_chainspec = {
            let mut ret = Chainspec::default();
            ret.network_config.name = chain_name.to_string();
            ret.core_config.pricing_handling = PricingHandling::Classic;
            ret
        };

        let current_timestamp = fixed_mode_transaction.timestamp();
        let expected_error = InvalidTransactionV1::InvalidPricingMode {
            price_mode: fixed_mode_transaction.pricing_mode().clone(),
        };

        assert_eq!(
            fixed_mode_transaction.is_config_compliant(
                &classic_handling_chainspec,
                TimeDiff::default(),
                current_timestamp,
            ),
            Err(expected_error)
        );
        assert!(
            fixed_mode_transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );

        assert!(fixed_mode_transaction
            .is_config_compliant(
                &fixed_handling_chainspec,
                TimeDiff::default(),
                current_timestamp,
            )
            .is_ok());

        let classic_mode_transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .with_pricing_mode(PricingMode::Classic {
                payment_amount: 100000,
                gas_price_tolerance: 1,
                standard_payment: true,
            })
            .build()
            .expect("must create classic transaction");

        let current_timestamp = classic_mode_transaction.timestamp();
        let expected_error = InvalidTransactionV1::InvalidPricingMode {
            price_mode: classic_mode_transaction.pricing_mode().clone(),
        };

        assert_eq!(
            classic_mode_transaction.is_config_compliant(
                &fixed_handling_chainspec,
                TimeDiff::default(),
                current_timestamp,
            ),
            Err(expected_error)
        );
        assert!(
            classic_mode_transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );

        assert!(classic_mode_transaction
            .is_config_compliant(
                &classic_handling_chainspec,
                TimeDiff::default(),
                current_timestamp,
            )
            .is_ok());
    }

    #[test]
    fn should_use_payment_amount_for_classic_payment() {
        let payment_amount = 500u64;
        let mut chainspec = Chainspec::default();
        let chain_name = "net-1";
        chainspec
            .with_chain_name(chain_name.to_string())
            .with_pricing_handling(PricingHandling::Classic);

        let rng = &mut TestRng::new();
        let builder = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .with_pricing_mode(PricingMode::Classic {
                payment_amount,
                gas_price_tolerance: 1,
                standard_payment: true,
            });
        let transaction = builder.build().expect("should build");
        let mut gas_price = 1;
        let cost = transaction
            .gas_cost(&chainspec, gas_price)
            .expect("should cost")
            .value();
        assert_eq!(
            cost,
            U512::from(payment_amount),
            "in classic pricing, the user selected amount should be the cost if gas price is 1"
        );
        gas_price += 1;
        let cost = transaction
            .gas_cost(&chainspec, gas_price)
            .expect("should cost")
            .value();
        assert_eq!(
            cost,
            U512::from(payment_amount) * gas_price,
            "in classic pricing, the cost should == user selected amount * gas_price"
        );
    }

    #[test]
    fn should_use_cost_table_for_fixed_payment() {
        let mut chainspec = Chainspec::default();
        let chain_name = "net-1";
        chainspec
            .with_chain_name(chain_name.to_string())
            .with_pricing_handling(PricingHandling::Fixed);

        let rng = &mut TestRng::new();
        let builder = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .with_pricing_mode(PricingMode::Fixed {
                gas_price_tolerance: 5,
            });
        let transaction = builder.build().expect("should build");
        let mut gas_price = 1;
        let limit = transaction
            .gas_limit(&chainspec)
            .expect("should limit")
            .value();
        let cost = transaction
            .gas_cost(&chainspec, gas_price)
            .expect("should cost")
            .value();
        assert_eq!(
            cost, limit,
            "in fixed pricing, the cost & limit should == if gas price is 1"
        );
        gas_price += 1;
        let cost = transaction
            .gas_cost(&chainspec, gas_price)
            .expect("should cost")
            .value();
        assert_eq!(
            cost,
            limit * gas_price,
            "in fixed pricing, the cost should == limit * gas_price"
        );
    }
}
