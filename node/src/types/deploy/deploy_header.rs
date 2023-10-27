use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::debug;

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    PublicKey, TimeDiff, Timestamp,
};

#[cfg(doc)]
use super::Deploy;
use super::{DeployConfigurationFailure, DeployHash};
use crate::{types::chainspec::DeployConfig, utils::DisplayIter};

/// The header portion of a [`Deploy`].
#[derive(
    Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
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

    /// Has this deploy expired?
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.expires() < current_instant
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

    /// Returns Ok if and only if the dependencies count and TTL are within limits, and the
    /// timestamp is not later than `at + timestamp_leeway`.  Does NOT check for expiry.
    pub fn is_valid(
        &self,
        config: &DeployConfig,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
        deploy_hash: &DeployHash,
    ) -> Result<(), DeployConfigurationFailure> {
        if self.dependencies.len() > config.max_dependencies as usize {
            debug!(
                %deploy_hash,
                deploy_header = %self,
                max_dependencies = %config.max_dependencies,
                "deploy dependency ceiling exceeded"
            );
            return Err(DeployConfigurationFailure::ExcessiveDependencies {
                max_dependencies: config.max_dependencies,
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
            return Err(DeployConfigurationFailure::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: self.ttl(),
            });
        }

        if self.timestamp() > at + timestamp_leeway {
            debug!(%deploy_hash, deploy_header = %self, %at, "deploy timestamp in the future");
            return Err(DeployConfigurationFailure::TimestampInFuture {
                validation_timestamp: at,
                timestamp_leeway,
                got: self.timestamp(),
            });
        }

        Ok(())
    }

    /// Returns the timestamp of when the deploy expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        self.timestamp.saturating_add(self.ttl)
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

#[cfg(test)]
impl DeployHeader {
    pub(super) fn invalidate(&mut self) {
        self.chain_name.clear();
    }
}
