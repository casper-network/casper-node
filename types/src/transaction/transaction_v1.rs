#[cfg(any(feature = "testing", test, feature = "json-schema"))]
pub(crate) mod arg_handling;
mod errors_v1;
pub mod fields_container;
mod transaction_args;
mod transaction_v1_hash;
pub mod transaction_v1_payload;

#[cfg(any(feature = "testing", test))]
use super::InitiatorAddrAndSecretKey;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::LARGE_WASM_LANE_ID;
use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes},
    crypto,
};
#[cfg(feature = "json-schema")]
use crate::{transaction::transaction_v1::fields_container::build_raw_payloads_map, PublicKey};
#[cfg(any(feature = "std", test))]
use crate::{
    TransactionEntryPoint, TransactionTarget, TransactionV1Config, AUCTION_LANE_ID,
    INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID,
};
#[cfg(feature = "json-schema")]
use crate::{TransactionScheduling, URef};
#[cfg(any(test, feature = "testing"))]
use alloc::collections::BTreeMap;
use alloc::{collections::BTreeSet, vec::Vec};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use errors_v1::FieldDeserializationError;
#[cfg(any(feature = "testing", test))]
use fields_container::FieldsContainer;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use fields_container::{ENTRY_POINT_MAP_KEY, TARGET_MAP_KEY};
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "std", test))]
use thiserror::Error;
use tracing::{error, trace};
pub use transaction_v1_payload::TransactionV1Payload;
#[cfg(any(feature = "std", test))]
use transaction_v1_payload::TransactionV1PayloadJson;

use super::{
    serialization::{CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder},
    Approval, ApprovalsHash, InitiatorAddr, PricingMode,
};
#[cfg(any(feature = "testing", test))]
use crate::bytesrepr::Bytes;
use crate::{Digest, DisplayIter, SecretKey, TimeDiff, Timestamp};

pub use errors_v1::{
    DecodeFromJsonErrorV1 as TransactionV1DecodeFromJsonError, ErrorV1 as TransactionV1Error,
    ExcessiveSizeErrorV1 as TransactionV1ExcessiveSizeError,
    InvalidTransaction as InvalidTransactionV1,
};
pub use transaction_args::TransactionArgs;
pub use transaction_v1_hash::TransactionV1Hash;

use core::{
    cmp,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

const HASH_FIELD_INDEX: u16 = 0;
const PAYLOAD_FIELD_INDEX: u16 = 1;
const APPROVALS_FIELD_INDEX: u16 = 2;

#[cfg(feature = "json-schema")]
pub(super) static TRANSACTION_V1: Lazy<TransactionV1> = Lazy::new(|| {
    let secret_key = SecretKey::example();
    let source = URef::from_formatted_str(
        "uref-0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a-007",
    )
    .unwrap();
    let target = URef::from_formatted_str(
        "uref-1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b-000",
    )
    .unwrap();
    let id = Some(999);
    let amount = 30_000_000_000_u64;
    let args = arg_handling::new_transfer_args(amount, Some(source), target, id).unwrap();
    let transaction_args = TransactionArgs::Named(args);
    let transaction_target = TransactionTarget::Native;
    let transaction_entry_point = TransactionEntryPoint::Transfer;
    let transaction_scheduling = TransactionScheduling::Standard;
    let pricing_mode = PricingMode::Fixed {
        gas_price_tolerance: 5,
        additional_computation_factor: 0,
    };
    let fields = build_raw_payloads_map(
        &transaction_args,
        &transaction_target,
        &transaction_entry_point,
        &transaction_scheduling,
    )
    .unwrap();
    let initiator_addr = InitiatorAddr::PublicKey(PublicKey::from(secret_key));
    let transaction_v1_payload = TransactionV1Payload::new(
        "casper-example".to_owned(),
        *Timestamp::example(),
        TimeDiff::from_seconds(3_600),
        pricing_mode,
        initiator_addr,
        fields,
    );
    let hash = Digest::hash(
        transaction_v1_payload
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
    );
    let mut transaction = TransactionV1::new(hash.into(), transaction_v1_payload, BTreeSet::new());
    transaction.sign(secret_key);
    transaction
});

/// A unit of work sent by a client to the network, which when executed can cause global state to
/// be altered.
#[derive(Clone, Eq, Debug)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(with = "TransactionV1Json")
)]
pub struct TransactionV1 {
    hash: TransactionV1Hash,
    payload: TransactionV1Payload,
    approvals: BTreeSet<Approval>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    is_verified: OnceCell<Result<(), InvalidTransactionV1>>,
}

#[cfg(any(feature = "std", test))]
impl TryFrom<TransactionV1Json> for TransactionV1 {
    type Error = TransactionV1JsonError;
    fn try_from(transaction_v1_json: TransactionV1Json) -> Result<Self, Self::Error> {
        Ok(TransactionV1 {
            hash: transaction_v1_json.hash,
            payload: transaction_v1_json.payload.try_into().map_err(|error| {
                TransactionV1JsonError::FailedToMap(format!(
                    "Failed to map TransactionJson::V1 to Transaction::V1, err: {}",
                    error
                ))
            })?,
            approvals: transaction_v1_json.approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        })
    }
}

/// A helper struct to represent the transaction as json.
#[cfg(any(feature = "std", test))]
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(
        description = "A unit of work sent by a client to the network, which when executed can \
        cause global state to be altered.",
        rename = "TransactionV1",
    )
)]
pub(super) struct TransactionV1Json {
    hash: TransactionV1Hash,
    payload: TransactionV1PayloadJson,
    approvals: BTreeSet<Approval>,
}

#[cfg(any(feature = "std", test))]
#[derive(Error, Debug)]
pub(super) enum TransactionV1JsonError {
    #[error("{0}")]
    FailedToMap(String),
}

#[cfg(any(feature = "std", test))]
impl TryFrom<TransactionV1> for TransactionV1Json {
    type Error = TransactionV1JsonError;
    fn try_from(transaction: TransactionV1) -> Result<Self, Self::Error> {
        Ok(TransactionV1Json {
            hash: transaction.hash,
            payload: transaction.payload.try_into().map_err(|error| {
                TransactionV1JsonError::FailedToMap(format!(
                    "Failed to map Transaction::V1 to TransactionJson::V1, err: {}",
                    error
                ))
            })?,
            approvals: transaction.approvals,
        })
    }
}

impl TransactionV1 {
    /// ctor
    pub fn new(
        hash: TransactionV1Hash,
        payload: TransactionV1Payload,
        approvals: BTreeSet<Approval>,
    ) -> Self {
        Self {
            hash,
            payload,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn build(
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        pricing_mode: PricingMode,
        fields: BTreeMap<u16, Bytes>,
        initiator_addr_and_secret_key: InitiatorAddrAndSecretKey,
    ) -> TransactionV1 {
        let initiator_addr = initiator_addr_and_secret_key.initiator_addr();
        let transaction_v1_payload = TransactionV1Payload::new(
            chain_name,
            timestamp,
            ttl,
            pricing_mode,
            initiator_addr,
            fields,
        );
        let hash = Digest::hash(
            transaction_v1_payload
                .to_bytes()
                .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
        );
        let mut transaction =
            TransactionV1::new(hash.into(), transaction_v1_payload, BTreeSet::new());

        if let Some(secret_key) = initiator_addr_and_secret_key.secret_key() {
            transaction.sign(secret_key);
        }
        transaction
    }

    /// Adds a signature of this transaction's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let approval = Approval::create(&self.hash.into(), secret_key);
        self.approvals.insert(approval);
    }

    /// Returns the `ApprovalsHash` of this transaction's approvals.
    pub fn hash(&self) -> &TransactionV1Hash {
        &self.hash
    }

    /// Returns the internal payload of this transaction.
    pub fn payload(&self) -> &TransactionV1Payload {
        &self.payload
    }

    /// Returns transactions approvals.
    pub fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }

    /// Returns the address of the initiator of the transaction.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        self.payload.initiator_addr()
    }

    /// Returns the name of the chain the transaction should be executed on.
    pub fn chain_name(&self) -> &str {
        self.payload.chain_name()
    }

    /// Returns the creation timestamp of the transaction.
    pub fn timestamp(&self) -> Timestamp {
        self.payload.timestamp()
    }

    /// Returns the duration after the creation timestamp for which the transaction will stay valid.
    ///
    /// After this duration has ended, the transaction will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.payload.ttl()
    }

    /// Returns `true` if the transaction has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.payload.expired(current_instant)
    }

    /// Returns the pricing mode for the transaction.
    pub fn pricing_mode(&self) -> &PricingMode {
        self.payload.pricing_mode()
    }

    /// Returns the `ApprovalsHash` of this transaction's approvals.
    pub fn compute_approvals_hash(&self) -> Result<ApprovalsHash, bytesrepr::Error> {
        ApprovalsHash::compute(&self.approvals)
    }

    #[doc(hidden)]
    pub fn with_approvals(mut self, approvals: BTreeSet<Approval>) -> Self {
        self.approvals = approvals;
        self
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn apply_approvals(&mut self, approvals: Vec<Approval>) {
        self.approvals.extend(approvals);
    }

    /// Returns the payment amount if the txn is using payment limited mode.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn payment_amount(&self) -> Option<u64> {
        if let PricingMode::PaymentLimited { payment_amount, .. } = self.pricing_mode() {
            Some(*payment_amount)
        } else {
            None
        }
    }

    /// Returns a random, valid but possibly expired transaction.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let ttl_millis = rng.gen_range(60_000..TimeDiff::from_seconds(2 * 60 * 60).millis());
        let timestamp = Timestamp::random(rng);
        let container = FieldsContainer::random(rng);
        let initiator_addr_and_secret_key = InitiatorAddrAndSecretKey::SecretKey(&secret_key);
        let pricing_mode = PricingMode::Fixed {
            gas_price_tolerance: 5,
            additional_computation_factor: 0,
        };
        TransactionV1::build(
            rng.random_string(5..10),
            timestamp,
            TimeDiff::from_millis(ttl_millis),
            pricing_mode,
            container.to_map().unwrap(),
            initiator_addr_and_secret_key,
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_lane_and_timestamp_and_ttl(
        rng: &mut TestRng,
        lane: u8,
        maybe_timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        let secret_key = SecretKey::random(rng);
        let timestamp = maybe_timestamp.unwrap_or_else(Timestamp::now);
        let ttl_millis = ttl.map_or(
            rng.gen_range(60_000..TimeDiff::from_seconds(2 * 60 * 60).millis()),
            |ttl| ttl.millis(),
        );
        let container = FieldsContainer::random_of_lane(rng, lane);
        let initiator_addr_and_secret_key = InitiatorAddrAndSecretKey::SecretKey(&secret_key);
        let pricing_mode = PricingMode::Fixed {
            gas_price_tolerance: 5,
            additional_computation_factor: 0,
        };
        TransactionV1::build(
            rng.random_string(5..10),
            timestamp,
            TimeDiff::from_millis(ttl_millis),
            pricing_mode,
            container.to_map().unwrap(),
            initiator_addr_and_secret_key,
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_timestamp_and_ttl(
        rng: &mut TestRng,
        maybe_timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        Self::random_with_lane_and_timestamp_and_ttl(
            rng,
            INSTALL_UPGRADE_LANE_ID,
            maybe_timestamp,
            ttl,
        )
    }

    /// Returns a random transaction with "transfer" category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_transfer(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        TransactionV1::random_with_lane_and_timestamp_and_ttl(rng, MINT_LANE_ID, timestamp, ttl)
    }

    /// Returns a random transaction with "standard" category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_wasm(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        TransactionV1::random_with_lane_and_timestamp_and_ttl(
            rng,
            LARGE_WASM_LANE_ID,
            timestamp,
            ttl,
        )
    }

    /// Returns a random transaction with "install/upgrade" category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_auction(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        TransactionV1::random_with_lane_and_timestamp_and_ttl(rng, AUCTION_LANE_ID, timestamp, ttl)
    }

    /// Returns a random transaction with "install/upgrade" category.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_install_upgrade(
        rng: &mut TestRng,
        timestamp: Option<Timestamp>,
        ttl: Option<TimeDiff>,
    ) -> Self {
        TransactionV1::random_with_lane_and_timestamp_and_ttl(
            rng,
            INSTALL_UPGRADE_LANE_ID,
            timestamp,
            ttl,
        )
    }

    /// Returns result of attempting to deserailize a field from the amorphic `fields` container.
    pub fn deserialize_field<T: FromBytes>(
        &self,
        index: u16,
    ) -> Result<T, FieldDeserializationError> {
        self.payload.deserialize_field(index)
    }

    /// Returns number of fields in the amorphic `fields` container.
    pub fn number_of_fields(&self) -> usize {
        self.payload.number_of_fields()
    }

    /// Checks if the declared hash of the transaction matches calculated hash.
    pub fn has_valid_hash(&self) -> Result<(), InvalidTransactionV1> {
        let computed_hash = Digest::hash(self.payload.to_bytes().map_err(|error| {
            error!(
                ?error,
                "Could not serialize transaction for purpose of calculating hash."
            );
            InvalidTransactionV1::CouldNotSerializeTransaction
        })?);
        if TransactionV1Hash::new(computed_hash) != self.hash {
            trace!(?self, ?computed_hash, "invalid transaction hash");
            return Err(InvalidTransactionV1::InvalidTransactionHash);
        }
        Ok(())
    }

    /// Returns `Ok` if and only if:
    ///   * the transaction hash is correct (see [`TransactionV1::has_valid_hash`] for details)
    ///   * approvals are non-empty, and
    ///   * all approvals are valid signatures of the signed hash
    pub fn verify(&self) -> Result<(), InvalidTransactionV1> {
        #[cfg(any(feature = "once_cell", test))]
        return self.is_verified.get_or_init(|| self.do_verify()).clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        self.do_verify()
    }

    fn do_verify(&self) -> Result<(), InvalidTransactionV1> {
        if self.approvals.is_empty() {
            trace!(?self, "transaction has no approvals");
            return Err(InvalidTransactionV1::EmptyApprovals);
        }

        self.has_valid_hash()?;

        for (index, approval) in self.approvals.iter().enumerate() {
            if let Err(error) = crypto::verify(self.hash, approval.signature(), approval.signer()) {
                trace!(
                    ?self,
                    "failed to verify transaction approval {}: {}",
                    index,
                    error
                );
                return Err(InvalidTransactionV1::InvalidApproval { index, error });
            }
        }

        Ok(())
    }

    /// Returns the hash of the transaction's payload.
    pub fn payload_hash(&self) -> Result<Digest, InvalidTransactionV1> {
        let bytes = self
            .payload
            .fields()
            .to_bytes()
            .map_err(|_| InvalidTransactionV1::CannotCalculateFieldsHash)?;
        Ok(Digest::hash(bytes))
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        vec![
            self.hash.serialized_length(),
            self.payload.serialized_length(),
            self.approvals.serialized_length(),
        ]
    }

    /// Turns `self` into an invalid `TransactionV1` by clearing the `chain_name`, invalidating the
    /// transaction hash
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        self.payload.invalidate();
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(crate) fn get_transaction_target(&self) -> Result<TransactionTarget, InvalidTransactionV1> {
        self.deserialize_field::<TransactionTarget>(TARGET_MAP_KEY)
            .map_err(|error| InvalidTransactionV1::CouldNotDeserializeField { error })
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(crate) fn get_transaction_entry_point(
        &self,
    ) -> Result<TransactionEntryPoint, InvalidTransactionV1> {
        self.deserialize_field::<TransactionEntryPoint>(ENTRY_POINT_MAP_KEY)
            .map_err(|error| InvalidTransactionV1::CouldNotDeserializeField { error })
    }

    /// Returns the gas price tolerance for the given transaction.
    pub fn gas_price_tolerance(&self) -> u8 {
        match self.pricing_mode() {
            PricingMode::PaymentLimited {
                gas_price_tolerance,
                ..
            } => *gas_price_tolerance,
            PricingMode::Fixed {
                gas_price_tolerance,
                ..
            } => *gas_price_tolerance,
            PricingMode::Prepaid { .. } => {
                // TODO: Change this when reserve gets implemented.
                0u8
            }
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &TRANSACTION_V1
    }
}

impl ToBytes for TransactionV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let expected_payload_sizes = self.serialized_field_lengths();
        CalltableSerializationEnvelopeBuilder::new(expected_payload_sizes)?
            .add_field(HASH_FIELD_INDEX, &self.hash)?
            .add_field(PAYLOAD_FIELD_INDEX, &self.payload)?
            .add_field(APPROVALS_FIELD_INDEX, &self.approvals)?
            .binary_payload_bytes()
    }

    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(3, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Error::Formatting)?;
        window.verify_index(HASH_FIELD_INDEX)?;
        let (hash, window) = window.deserialize_and_maybe_next::<TransactionV1Hash>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(PAYLOAD_FIELD_INDEX)?;
        let (payload, window) = window.deserialize_and_maybe_next::<TransactionV1Payload>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(APPROVALS_FIELD_INDEX)?;
        let (approvals, window) = window.deserialize_and_maybe_next::<BTreeSet<Approval>>()?;
        if window.is_some() {
            return Err(Error::Formatting);
        }
        let from_bytes = TransactionV1 {
            hash,
            payload,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };
        Ok((from_bytes, remainder))
    }
}

impl Display for TransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-v1[{}, {}, approvals: {}]",
            self.hash,
            self.payload,
            DisplayIter::new(self.approvals.iter())
        )
    }
}

impl hash::Hash for TransactionV1 {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
            hash,
            payload,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        hash.hash(state);
        payload.hash(state);
        approvals.hash(state);
    }
}

impl PartialEq for TransactionV1 {
    fn eq(&self, other: &TransactionV1) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
            hash,
            payload,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        *hash == other.hash && *payload == other.payload && *approvals == other.approvals
    }
}

impl Ord for TransactionV1 {
    fn cmp(&self, other: &TransactionV1) -> cmp::Ordering {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
            hash,
            payload,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        hash.cmp(&other.hash)
            .then_with(|| payload.cmp(&other.payload))
            .then_with(|| approvals.cmp(&other.approvals))
    }
}

impl PartialOrd for TransactionV1 {
    fn partial_cmp(&self, other: &TransactionV1) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(any(feature = "std", test))]
/// Calculates the laned based on properties of the transaction
pub fn calculate_transaction_lane(
    entry_point: &TransactionEntryPoint,
    target: &TransactionTarget,
    pricing_mode: &PricingMode,
    config: &TransactionV1Config,
    size_estimation: u64,
    runtime_args_size: u64,
) -> Result<u8, InvalidTransactionV1> {
    use crate::TransactionRuntimeParams;

    use super::get_lane_for_non_install_wasm;

    match target {
        TransactionTarget::Native => match entry_point {
            TransactionEntryPoint::Transfer | TransactionEntryPoint::Burn => Ok(MINT_LANE_ID),
            TransactionEntryPoint::AddBid
            | TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => Ok(AUCTION_LANE_ID),
            TransactionEntryPoint::Call => Err(InvalidTransactionV1::EntryPointCannotBeCall),
            TransactionEntryPoint::Custom(_) => {
                Err(InvalidTransactionV1::EntryPointCannotBeCustom {
                    entry_point: entry_point.clone(),
                })
            }
        },
        TransactionTarget::Stored { .. } => match entry_point {
            TransactionEntryPoint::Custom(_) => get_lane_for_non_install_wasm(
                config,
                pricing_mode,
                size_estimation,
                runtime_args_size,
            )
            .map_err(Into::into),
            TransactionEntryPoint::Call
            | TransactionEntryPoint::Transfer
            | TransactionEntryPoint::Burn
            | TransactionEntryPoint::AddBid
            | TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => {
                Err(InvalidTransactionV1::EntryPointMustBeCustom {
                    entry_point: entry_point.clone(),
                })
            }
        },
        TransactionTarget::Session {
            is_install_upgrade,
            runtime: TransactionRuntimeParams::VmCasperV1,
            ..
        } => match entry_point {
            TransactionEntryPoint::Call => {
                if *is_install_upgrade {
                    Ok(INSTALL_UPGRADE_LANE_ID)
                } else {
                    get_lane_for_non_install_wasm(
                        config,
                        pricing_mode,
                        size_estimation,
                        runtime_args_size,
                    )
                    .map_err(Into::into)
                }
            }
            TransactionEntryPoint::Custom(_)
            | TransactionEntryPoint::Transfer
            | TransactionEntryPoint::Burn
            | TransactionEntryPoint::AddBid
            | TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => {
                Err(InvalidTransactionV1::EntryPointMustBeCall {
                    entry_point: entry_point.clone(),
                })
            }
        },
        TransactionTarget::Session {
            is_install_upgrade,
            runtime: TransactionRuntimeParams::VmCasperV2 { .. },
            ..
        } => match entry_point {
            TransactionEntryPoint::Call | TransactionEntryPoint::Custom(_) => {
                if *is_install_upgrade {
                    Ok(INSTALL_UPGRADE_LANE_ID)
                } else {
                    get_lane_for_non_install_wasm(
                        config,
                        pricing_mode,
                        size_estimation,
                        runtime_args_size,
                    )
                    .map_err(Into::into)
                }
            }
            TransactionEntryPoint::Transfer
            | TransactionEntryPoint::Burn
            | TransactionEntryPoint::AddBid
            | TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => {
                Err(InvalidTransactionV1::EntryPointMustBeCall {
                    entry_point: entry_point.clone(),
                })
            }
        },
    }
}
