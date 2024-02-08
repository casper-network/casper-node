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
use super::Deploy;
use super::DeployHash;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, DisplayIter, PublicKey, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{DeployConfigFailure, TransactionConfig};

/// The header portion of a [`Deploy`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
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
    #[cfg(any(feature = "std", feature = "json-schema", test))]
    pub(super) fn new(
        account: PublicKey,
        timestamp: Timestamp,
        ttl: TimeDiff,
        gas_price: u64,
        body_hash: Digest,
        dependencies: Vec<DeployHash>,
        chain_name: String,
    ) -> Self {
        DeployHeader {
            account,
            timestamp,
            ttl,
            gas_price,
            body_hash,
            dependencies,
            chain_name,
        }
    }

    /// Returns the public key of the account providing the context in which to run the `Deploy`.
    pub fn account(&self) -> &PublicKey {
        &self.account
    }

    /// Returns the creation timestamp of the `Deploy`.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the duration after the creation timestamp for which the `Deploy` will stay valid.
    ///
    /// After this duration has ended, the `Deploy` will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    /// Returns `true` if the `Deploy` has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.expires() < current_instant
    }

    /// Returns the price per gas unit for the `Deploy`.
    pub fn gas_price(&self) -> u64 {
        self.gas_price
    }

    /// Returns the hash of the body (i.e. the Wasm code) of the `Deploy`.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// Returns the list of other `Deploy`s that have to be executed before this one.
    pub fn dependencies(&self) -> &Vec<DeployHash> {
        &self.dependencies
    }

    /// Returns the name of the chain the `Deploy` should be executed on.
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// Returns `Ok` if and only if the dependencies count and TTL are within limits, and the
    /// timestamp is not later than `at + timestamp_leeway`.  Does NOT check for expiry.
    #[cfg(any(feature = "std", test))]
    pub fn is_valid(
        &self,
        config: &TransactionConfig,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
        deploy_hash: &DeployHash,
    ) -> Result<(), DeployConfigFailure> {
        if self.dependencies.len() > config.deploy_config.max_dependencies as usize {
            debug!(
                %deploy_hash,
                deploy_header = %self,
                max_dependencies = %config.deploy_config.max_dependencies,
                "deploy dependency ceiling exceeded"
            );
            return Err(DeployConfigFailure::ExcessiveDependencies {
                max_dependencies: config.deploy_config.max_dependencies,
                got: self.dependencies().len(),
            });
        }

        if self.ttl() > config.max_ttl {
            debug!(
                %deploy_hash,
                deploy_header = %self,
                max_ttl = %config.max_ttl,
                "deploy ttl excessive"
            );
            return Err(DeployConfigFailure::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: self.ttl(),
            });
        }

        if self.timestamp() > at + timestamp_leeway {
            debug!(%deploy_hash, deploy_header = %self, %at, "deploy timestamp in the future");
            return Err(DeployConfigFailure::TimestampInFuture {
                validation_timestamp: at,
                timestamp_leeway,
                got: self.timestamp(),
            });
        }

        Ok(())
    }

    /// Returns the timestamp of when the `Deploy` expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp.saturating_add(self.ttl)
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn invalidate(&mut self) {
        self.chain_name.clear();
    }
}

impl ToBytes for DeployHeader {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.account.write_bytes(writer)?;
        self.timestamp.write_bytes(writer)?;
        self.ttl.write_bytes(writer)?;
        self.gas_price.write_bytes(writer)?;
        self.body_hash.write_bytes(writer)?;
        self.dependencies.write_bytes(writer)?;
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
            + self.gas_price.serialized_length()
            + self.body_hash.serialized_length()
            + self.dependencies.serialized_length()
            + self.chain_name.serialized_length()
    }
}

impl FromBytes for DeployHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account, remainder) = PublicKey::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (gas_price, remainder) = u64::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (dependencies, remainder) = Vec::<DeployHash>::from_bytes(remainder)?;
        let (chain_name, remainder) = String::from_bytes(remainder)?;
        let deploy_header = DeployHeader {
            account,
            timestamp,
            ttl,
            gas_price,
            body_hash,
            dependencies,
            chain_name,
        };
        Ok((deploy_header, remainder))
    }
}

impl Display for DeployHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy-header[account: {}, timestamp: {}, ttl: {}, gas_price: {}, body_hash: {}, \
            dependencies: [{}], chain_name: {}]",
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
