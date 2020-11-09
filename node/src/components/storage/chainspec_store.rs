use semver::Version;

use super::Result;
use crate::Chainspec;

/// Trait defining the API for a chainspec store managed by the storage component.
pub trait ChainspecStore: Send + Sync {
    fn put(&self, chainspec: Chainspec) -> Result<()>;
    fn get(&self, version: Version) -> Result<Option<Chainspec>>;
}

#[cfg(test)]
mod tests {
    use super::{
        super::{Config, InMemChainspecStore, LmdbChainspecStore},
        *,
    };

    fn should_put_then_get<T: ChainspecStore>(chainspec_store: &mut T) {
        let mut rng = crate::new_rng();

        let chainspec = Chainspec::random(&mut rng);
        let version = chainspec.genesis.protocol_version.clone();

        chainspec_store.put(chainspec.clone()).unwrap();
        let maybe_chainspec = chainspec_store.get(version).unwrap();
        let recovered_chainspec = maybe_chainspec.unwrap();

        assert_eq!(recovered_chainspec, chainspec);
    }

    #[test]
    fn lmdb_chainspec_store_should_put_then_get() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_chainspec_store =
            LmdbChainspecStore::new(config.path(), config.max_chainspec_store_size()).unwrap();
        should_put_then_get(&mut lmdb_chainspec_store);
    }

    #[test]
    fn in_mem_chainspec_store_should_put_then_get() {
        let mut in_mem_chainspec_store = InMemChainspecStore::new();
        should_put_then_get(&mut in_mem_chainspec_store);
    }

    fn should_fail_get<T: ChainspecStore>(chainspec_store: &mut T) {
        let mut rng = crate::new_rng();

        let chainspec = Chainspec::random(&mut rng);
        let mut version = chainspec.genesis.protocol_version.clone();
        version.patch += 1;

        chainspec_store.put(chainspec).unwrap();
        assert!(chainspec_store.get(version).unwrap().is_none());
    }

    #[test]
    fn lmdb_chainspec_store_should_fail_to_get_unknown_version() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_chainspec_store =
            LmdbChainspecStore::new(config.path(), config.max_chainspec_store_size()).unwrap();
        should_fail_get(&mut lmdb_chainspec_store);
    }

    #[test]
    fn in_mem_chainspec_store_should_fail_to_get_unknown_version() {
        let mut in_mem_chainspec_store = InMemChainspecStore::new();
        should_fail_get(&mut in_mem_chainspec_store);
    }
}
