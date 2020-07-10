use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::RwLock,
};

use semver::Version;
use thiserror::Error;

use super::{ChainspecStore, Error, Result};
use crate::Chainspec;

#[derive(Error, Debug)]
#[error("poisoned lock")]
pub(super) struct PoisonedLock {}

/// In-memory version of a store.
#[derive(Debug)]
pub(super) struct InMemChainspecStore {
    inner: RwLock<HashMap<Version, Chainspec>>,
}

impl InMemChainspecStore {
    pub(crate) fn new() -> Self {
        InMemChainspecStore {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl ChainspecStore for InMemChainspecStore {
    fn put(&self, chainspec: Chainspec) -> Result<()> {
        if let Entry::Vacant(entry) = self
            .inner
            .write()?
            .entry(chainspec.genesis.protocol_version.clone())
        {
            entry.insert(chainspec);
        }
        Ok(())
    }

    fn get(&self, version: Version) -> Result<Chainspec> {
        self.inner
            .read()?
            .get(&version)
            .cloned()
            .ok_or_else(|| Error::NotFound)
    }
}
