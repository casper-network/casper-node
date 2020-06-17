use super::{Result, Value};

/// Trait defining the API for a store managed by the storage component.
pub trait Store: Send + Sync {
    type Value: Value;

    fn put(&self, block: Self::Value) -> Result<()>;
    fn get(&self, id: &<Self::Value as Value>::Id) -> Result<Self::Value>;
    fn get_header(&self, id: &<Self::Value as Value>::Id)
        -> Result<<Self::Value as Value>::Header>;
    /// Returns a copy of all IDs held by the store.
    fn ids(&self) -> Result<Vec<<Self::Value as Value>::Id>>;
}
