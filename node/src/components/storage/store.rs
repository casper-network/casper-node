use smallvec::SmallVec;

use super::{Result, Value};

pub(super) type Multiple<T> = SmallVec<[T; 3]>;

/// Trait defining the API for a store managed by the storage component.
pub trait Store: Send + Sync {
    type Value: Value;

    /// If the store did not have this value present, true is returned.  If the store did have this
    /// value present, false is returned.
    fn put(&self, block: Self::Value) -> Result<bool>;
    fn get(
        &self,
        ids: Multiple<<Self::Value as Value>::Id>,
    ) -> Multiple<Result<Option<Self::Value>>>;
    fn get_headers(
        &self,
        ids: Multiple<<Self::Value as Value>::Id>,
    ) -> Multiple<Result<Option<<Self::Value as Value>::Header>>>;
    /// Returns a copy of all IDs held by the store.
    fn ids(&self) -> Result<Vec<<Self::Value as Value>::Id>>;
}

#[cfg(test)]
mod tests {
    use smallvec::smallvec;

    use super::{
        super::{Config, InMemStore, LmdbStore},
        *,
    };
    use crate::{testing::TestRng, types::Deploy};

    fn should_put_then_get<T: Store<Value = Deploy>>(store: &mut T) {
        let mut rng = TestRng::new();

        let deploy = Deploy::random(&mut rng);
        let deploy_hash = *deploy.id();

        store.put(deploy.clone()).unwrap();
        let maybe_deploy = store
            .get(smallvec![deploy_hash])
            .pop()
            .expect("should be only one")
            .expect("get should return Ok");
        let recovered_deploy = maybe_deploy.unwrap();

        assert_eq!(recovered_deploy, deploy);
    }

    #[test]
    fn lmdb_deploy_store_should_put_then_get() {
        let (config, _tempdir) = Config::default_for_tests();
        let mut lmdb_deploy_store =
            LmdbStore::<Deploy>::new(config.path(), config.max_deploy_store_size()).unwrap();
        should_put_then_get(&mut lmdb_deploy_store);
    }

    #[test]
    fn in_mem_deploy_store_should_put_then_get() {
        let mut in_mem_deploy_store = InMemStore::<Deploy>::new();
        should_put_then_get(&mut in_mem_deploy_store);
    }
}
