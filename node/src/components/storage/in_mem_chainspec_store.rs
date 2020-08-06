use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::RwLock,
};

use semver::Version;

use super::{ChainspecStore, Result};
use crate::Chainspec;

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
            .write()
            .expect("should lock")
            .entry(chainspec.genesis.protocol_version.clone())
        {
            entry.insert(chainspec);
        }
        Ok(())
    }

    fn get(&self, version: Version) -> Result<Option<Chainspec>> {
        Ok(self
            .inner
            .read()
            .expect("should lock")
            .get(&version)
            .cloned())
    }
}
