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

use super::PricingModeV1;
#[cfg(doc)]
use super::TransactionV1;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, PublicKey, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{TransactionConfig, TransactionV1ConfigFailure, TransactionV1Hash};

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
    account: PublicKey,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingModeV1,
    body_hash: Digest,
    chain_name: String,
}

impl TransactionV1Header {
    #[cfg(any(feature = "std", feature = "json-schema", test))]
    pub(super) fn new(
        account: PublicKey,
        timestamp: Timestamp,
        ttl: TimeDiff,
        pricing_mode: PricingModeV1,
        body_hash: Digest,
        chain_name: String,
    ) -> Self {
        TransactionV1Header {
            account,
            timestamp,
            ttl,
            pricing_mode,
            body_hash,
            chain_name,
        }
    }

    /// Returns the public key of the account providing the context in which to run the transaction.
    pub fn account(&self) -> &PublicKey {
        &self.account
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

    /// Returns the pricing mode for the transaction.
    pub fn pricing_mode(&self) -> &PricingModeV1 {
        &self.pricing_mode
    }

    /// Returns the hash of the body of the transaction.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// Returns the name of the chain the transaction should be executed on.
    pub fn chain_name(&self) -> &str {
        &self.chain_name
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
    ) -> Result<(), TransactionV1ConfigFailure> {
        if self.ttl() > config.max_ttl {
            debug!(
                %transaction_hash,
                transaction_header = %self,
                max_ttl = %config.max_ttl,
                "transaction ttl excessive"
            );
            return Err(TransactionV1ConfigFailure::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: self.ttl(),
            });
        }

        if self.timestamp() > at + timestamp_leeway {
            debug!(
                %transaction_hash, transaction_header = %self, %at,
                "transaction timestamp in the future"
            );
            return Err(TransactionV1ConfigFailure::TimestampInFuture {
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

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn invalidate(&mut self) {
        self.chain_name.clear();
    }
}

impl ToBytes for TransactionV1Header {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.account.write_bytes(writer)?;
        self.timestamp.write_bytes(writer)?;
        self.ttl.write_bytes(writer)?;
        self.pricing_mode.write_bytes(writer)?;
        self.body_hash.write_bytes(writer)?;
        self.chain_name.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.account.serialized_length()
            + self.timestamp.serialized_length()
            + self.ttl.serialized_length()
            + self.pricing_mode.serialized_length()
            + self.body_hash.serialized_length()
            + self.chain_name.serialized_length()
    }
}

impl FromBytes for TransactionV1Header {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account, remainder) = PublicKey::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (pricing_mode, remainder) = PricingModeV1::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (chain_name, remainder) = String::from_bytes(remainder)?;
        let transaction_header = TransactionV1Header {
            account,
            timestamp,
            ttl,
            pricing_mode,
            body_hash,
            chain_name,
        };
        Ok((transaction_header, remainder))
    }
}

impl Display for TransactionV1Header {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-v1-header[account: {}, timestamp: {}, ttl: {}, pricing mode: {}, \
            chain_name: {}]",
            self.account, self.timestamp, self.ttl, self.pricing_mode, self.chain_name,
        )
    }
}
