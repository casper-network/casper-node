use smallvec::SmallVec;

use super::{DeployAndMetadata, Result, Value};
use crate::types::json_compatibility::ExecutionResult;

pub(super) type Multiple<T> = SmallVec<[T; 3]>;

/// Trait defining the API for a store managed by the storage component.
pub trait Store: Send + Sync {
    type Value: Value;

    /// If the store did not have this value present, true is returned.  If the store did have this
    /// value present, false is returned.
    fn put(&self, value: Self::Value) -> Result<bool>;
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

pub trait DeployStore: Store {
    type Block: Value;
    type Deploy: Value;

    fn put_execution_result(
        &self,
        id: <Self::Deploy as Value>::Id,
        block_hash: <Self::Block as Value>::Id,
        execution_result: ExecutionResult,
    ) -> Result<bool>;

    /// Returns the deploy and its associated metadata if the deploy exists.
    fn get_deploy_and_metadata(
        &self,
        id: <Self::Deploy as Value>::Id,
    ) -> Result<Option<DeployAndMetadata<Self::Deploy, Self::Block>>>;
}

#[cfg(test)]
mod tests {
    use smallvec::smallvec;

    use super::{
        super::{Config, DeployMetadata, InMemStore, LmdbStore},
        *,
    };
    use crate::{
        testing::TestRng,
        types::{Block, Deploy},
    };

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
        let mut lmdb_deploy_store = LmdbStore::<Deploy, DeployMetadata<Block>>::new(
            config.path(),
            config.max_deploy_store_size(),
        )
        .unwrap();
        should_put_then_get(&mut lmdb_deploy_store);
    }

    #[test]
    fn in_mem_deploy_store_should_put_then_get() {
        let mut in_mem_deploy_store = InMemStore::<Deploy, DeployMetadata<Block>>::new();
        should_put_then_get(&mut in_mem_deploy_store);
    }
}
