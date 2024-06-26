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
use super::{InitiatorAddr, PricingMode};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    transaction::serialization::{TransactionV1HeaderFromBytes, TransactionV1HeaderToBytes},
    Digest, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{InvalidTransactionV1, TransactionConfig, TransactionV1Hash};

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
            PricingMode::Classic {
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
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        TransactionV1HeaderToBytes::new(
            &self.chain_name,
            &self.timestamp,
            &self.ttl,
            &self.body_hash,
            &self.pricing_mode,
            &self.initiator_addr,
        )
        .to_bytes()
    }

    fn serialized_length(&self) -> usize {
        TransactionV1HeaderToBytes::new(
            &self.chain_name,
            &self.timestamp,
            &self.ttl,
            &self.body_hash,
            &self.pricing_mode,
            &self.initiator_addr,
        )
        .serialized_length()
    }
}

impl FromBytes for TransactionV1Header {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (binary_header, remainder) = TransactionV1HeaderFromBytes::from_bytes(bytes)?;
        let header = TransactionV1Header {
            chain_name: binary_header.chain_name,
            timestamp: binary_header.timestamp,
            ttl: binary_header.ttl,
            body_hash: binary_header.body_hash,
            pricing_mode: binary_header.pricing_mode,
            initiator_addr: binary_header.initiator_addr,
        };
        Ok((header, remainder))
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
