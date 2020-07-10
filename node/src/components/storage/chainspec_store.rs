use semver::Version;

use super::Result;
use crate::Chainspec;

/// Trait defining the API for a chainspec store managed by the storage component.
pub trait ChainspecStore: Send + Sync {
    fn put(&self, chainspec: Chainspec) -> Result<()>;
    fn get(&self, version: Version) -> Result<Chainspec>;
}
