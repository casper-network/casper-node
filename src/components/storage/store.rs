use std::error::Error as StdError;

use serde::{de::DeserializeOwned, Serialize};

use super::Value;

/// Trait defining the API for a store managed by the storage component.
pub(crate) trait Store {
    type Value: Value;
    type Error: StdError + Clone + Serialize + DeserializeOwned + Send + Sync;

    fn put(&self, block: Self::Value) -> Result<(), Self::Error>;
    fn get(&self, id: &<Self::Value as Value>::Id) -> Result<Self::Value, Self::Error>;
    fn get_header(
        &self,
        id: &<Self::Value as Value>::Id,
    ) -> Result<<Self::Value as Value>::Header, Self::Error>;
}
