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

use super::PricingMode;
#[cfg(doc)]
use super::Transaction;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, PublicKey, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{TransactionConfig, TransactionConfigFailure, TransactionHash};

/// The header portion of a [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct TransactionHeader {
    account: PublicKey,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingMode,
    body_hash: Digest,
    chain_name: String,
}

impl TransactionHeader {
    #[cfg(any(feature = "std", feature = "json-schema", test))]
    pub(super) fn new(
        account: PublicKey,
        timestamp: Timestamp,
        ttl: TimeDiff,
        pricing_mode: PricingMode,
        body_hash: Digest,
        chain_name: String,
    ) -> Self {
        TransactionHeader {
            account,
            timestamp,
            ttl,
            pricing_mode,
            body_hash,
            chain_name,
        }
    }

    /// Returns the public key of the account providing the context in which to run the
    /// `Transaction`.
    pub fn account(&self) -> &PublicKey {
        &self.account
    }

    /// Returns the creation timestamp of the `Transaction`.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the duration after the creation timestamp for which the `Transaction` will stay
    /// valid.
    ///
    /// After this duration has ended, the `Transaction` will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    /// Returns `true` if the `Transaction` has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.expires() < current_instant
    }

    /// Returns the pricing mode for the `Transaction`.
    pub fn pricing_mode(&self) -> &PricingMode {
        &self.pricing_mode
    }

    /// Returns the hash of the body of the `Transaction`.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// Returns the name of the chain the `Transaction` should be executed on.
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// Returns `Ok` if and only if the TTL is within limits, and the timestamp is not later than
    /// `at`.  Does NOT check for expiry.
    #[cfg(any(feature = "std", test))]
    pub fn is_valid(
        &self,
        config: &TransactionConfig,
        at: Timestamp,
        transaction_hash: &TransactionHash,
    ) -> Result<(), TransactionConfigFailure> {
        if self.ttl() > config.max_ttl {
            debug!(
                %transaction_hash,
                transaction_header = %self,
                max_ttl = %config.max_ttl,
                "transaction ttl excessive"
            );
            return Err(TransactionConfigFailure::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: self.ttl(),
            });
        }

        if self.timestamp() > at {
            debug!(
                %transaction_hash, transaction_header = %self, %at,
                "transaction timestamp in the future"
            );
            return Err(TransactionConfigFailure::TimestampInFuture {
                validation_timestamp: at,
                got: self.timestamp(),
            });
        }

        Ok(())
    }

    /// Returns the timestamp of when the `Transaction` expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp.saturating_add(self.ttl)
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn invalidate(&mut self) {
        self.chain_name.clear();
    }
}

impl ToBytes for TransactionHeader {
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

impl FromBytes for TransactionHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account, remainder) = PublicKey::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (pricing_mode, remainder) = PricingMode::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (chain_name, remainder) = String::from_bytes(remainder)?;
        let transaction_header = TransactionHeader {
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

impl Display for TransactionHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-header[account: {}, timestamp: {}, ttl: {}, pricing mode: {}, \
            chain_name: {}]",
            self.account, self.timestamp, self.ttl, self.pricing_mode, self.chain_name,
        )
    }
}
