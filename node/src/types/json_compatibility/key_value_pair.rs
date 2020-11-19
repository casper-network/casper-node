use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Intemediary data structure to help serialize Operations and Transforms to OpenRPC compatible
/// output
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Default, Debug, DataSize)]
pub struct KeyValuePair<K, V>
where
    K: Serialize,
    V: Serialize,
{
    key: K,
    value: V,
}

impl<K, V> KeyValuePair<K, V>
where
    K: Serialize,
    V: Serialize,
{
    /// Create a new instance
    pub fn new(key: K, value: V) -> Self {
        KeyValuePair { key, value }
    }
}

/*
impl<K,V> Display for KeyValuePair<K,V>
where
    K: Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.value)
    }
}
*/
