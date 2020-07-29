use smallvec::SmallVec;

use super::{Result, Value};

pub(super) type Multiple<T> = SmallVec<[T; 3]>;

/// Trait defining the API for a store managed by the storage component.
pub trait Store: Send + Sync {
    type Value: Value;

    /// If the store did not have this value present, true is returned.  If the store did have this
    /// value present, false is returned.
    fn put(&self, block: Self::Value) -> Result<bool>;
    fn get(&self, ids: Multiple<<Self::Value as Value>::Id>) -> Multiple<Result<Self::Value>>;
    fn get_headers(
        &self,
        ids: Multiple<<Self::Value as Value>::Id>,
    ) -> Multiple<Result<<Self::Value as Value>::Header>>;
    /// Returns a copy of all IDs held by the store.
    fn ids(&self) -> Result<Vec<<Self::Value as Value>::Id>>;
}
