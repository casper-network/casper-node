use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "std", test))]
use tracing::debug;

#[cfg(doc)]
use super::TransactionV1;
#[cfg(any(feature = "std", test))]
use super::TransactionV1Hash;
use super::{InitiatorAddr, PricingMode};
use crate::{
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::{
        CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder,
    },
    Digest, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{InvalidTransactionV1, TransactionConfig};

const CHAIN_NAME_INDEX: u16 = 0;
const TIMESTAMP_INDEX: u16 = 1;
const TTL_INDEX: u16 = 2;
const BODY_HASH_INDEX: u16 = 3;
const PRICING_MODE_INDEX: u16 = 4;
const INITIATOR_ADDR_INDEX: u16 = 5;

/// The header portion of a [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The header portion of a TransactionV1.")
)]
pub struct TransactionV1Header {
    chain_name: String,
    timestamp: Timestamp,
    ttl: TimeDiff,
    body_hash: Digest,
    pricing_mode: PricingMode,
    initiator_addr: InitiatorAddr,
}

impl TransactionV1Header {
    fn serialized_field_lengths(&self) -> Vec<usize> {
        vec![
            self.chain_name.serialized_length(),
            self.timestamp.serialized_length(),
            self.ttl.serialized_length(),
            self.body_hash.serialized_length(),
            self.pricing_mode.serialized_length(),
            self.initiator_addr.serialized_length(),
        ]
    }

    #[cfg(any(feature = "std", feature = "json-schema", test))]
    pub(super) fn new(
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        body_hash: Digest,
        pricing_mode: PricingMode,
        initiator_addr: InitiatorAddr,
    ) -> Self {
        TransactionV1Header {
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            initiator_addr,
        }
    }

    /// Computes the hash identifying this transaction.
    #[cfg(any(feature = "std", test))]
    pub fn compute_hash(&self) -> TransactionV1Hash {
        TransactionV1Hash::new(Digest::hash(
            self.to_bytes()
                .unwrap_or_else(|error| panic!("should serialize header: {}", error)),
        ))
    }

    /// Returns the name of the chain the transaction should be executed on.
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// Returns the creation timestamp of the transaction.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the duration after the creation timestamp for which the transaction will stay valid.
    ///
    /// After this duration has ended, the transaction will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    /// Returns `true` if the transaction has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.expires() < current_instant
    }

    /// Returns the hash of the body of the transaction.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// Returns the pricing mode for the transaction.
    #[inline]
    pub fn pricing_mode(&self) -> &PricingMode {
        &self.pricing_mode
    }

    /// Returns the address of the initiator of the transaction.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// Returns `Ok` if and only if the TTL is within limits, and the timestamp is not later than
    /// `at + timestamp_leeway`.  Does NOT check for expiry.
    #[cfg(any(feature = "std", test))]
    pub fn is_valid(
        &self,
        config: &TransactionConfig,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
        transaction_hash: &TransactionV1Hash,
    ) -> Result<(), InvalidTransactionV1> {
        if self.ttl() > config.max_ttl {
            debug!(
                %transaction_hash,
                transaction_header = %self,
                max_ttl = %config.max_ttl,
                "transaction ttl excessive"
            );
            return Err(InvalidTransactionV1::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: self.ttl(),
            });
        }

        if self.timestamp() > at + timestamp_leeway {
            debug!(
                %transaction_hash, transaction_header = %self, %at,
                "transaction timestamp in the future"
            );
            return Err(InvalidTransactionV1::TimestampInFuture {
                validation_timestamp: at,
                timestamp_leeway,
                got: self.timestamp(),
            });
        }

        Ok(())
    }

    /// Returns the timestamp of when the transaction expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp.saturating_add(self.ttl)
    }

    /// Returns the gas price tolerance for the given transaction.
    pub fn gas_price_tolerance(&self) -> u8 {
        match self.pricing_mode {
            PricingMode::PaymentLimited {
                gas_price_tolerance,
                ..
            } => gas_price_tolerance,
            PricingMode::Fixed {
                gas_price_tolerance,
                ..
            } => gas_price_tolerance,
            PricingMode::Reserved { .. } => {
                // TODO: Change this when reserve gets implemented.
                0u8
            }
        }
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn invalidate(&mut self) {
        self.chain_name.clear();
    }
}

impl ToBytes for TransactionV1Header {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
            .add_field(CHAIN_NAME_INDEX, &self.chain_name)?
            .add_field(TIMESTAMP_INDEX, &self.timestamp)?
            .add_field(TTL_INDEX, &self.ttl)?
            .add_field(BODY_HASH_INDEX, &self.body_hash)?
            .add_field(PRICING_MODE_INDEX, &self.pricing_mode)?
            .add_field(INITIATOR_ADDR_INDEX, &self.initiator_addr)?
            .binary_payload_bytes()
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionV1Header {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionV1Header, &[u8]), Error> {
        let (binary_payload, remainder) =
            crate::transaction::serialization::CalltableSerializationEnvelope::from_bytes(
                6u32, bytes,
            )?;
        let window = binary_payload.start_consuming()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(CHAIN_NAME_INDEX)?;
        let (chain_name, window) = window.deserialize_and_maybe_next::<String>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(TIMESTAMP_INDEX)?;
        let (timestamp, window) = window.deserialize_and_maybe_next::<Timestamp>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(TTL_INDEX)?;
        let (ttl, window) = window.deserialize_and_maybe_next::<TimeDiff>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(BODY_HASH_INDEX)?;
        let (body_hash, window) = window.deserialize_and_maybe_next::<Digest>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(PRICING_MODE_INDEX)?;
        let (pricing_mode, window) = window.deserialize_and_maybe_next::<PricingMode>()?;
        let window = window.ok_or(Formatting)?;
        window.verify_index(INITIATOR_ADDR_INDEX)?;
        let (initiator_addr, window) = window.deserialize_and_maybe_next::<InitiatorAddr>()?;
        if window.is_some() {
            return Err(Formatting);
        }
        let from_bytes = TransactionV1Header {
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            initiator_addr,
        };
        Ok((from_bytes, remainder))
    }
}

impl Display for TransactionV1Header {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        #[cfg(any(feature = "std", test))]
        let hash = self.compute_hash();
        #[cfg(not(any(feature = "std", test)))]
        let hash = "unknown";
        write!(
            formatter,
            "transaction-v1-header[{}, chain_name: {}, timestamp: {}, ttl: {}, pricing mode: {}, \
            initiator: {}]",
            hash, self.chain_name, self.timestamp, self.ttl, self.pricing_mode, self.initiator_addr
        )
    }
}
