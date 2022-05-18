use std::collections::BTreeMap;

use casper_types::bytesrepr::{FromBytes, ToBytes};

use crate::storage::store::{Store, StoreExt};

// should be moved to the `store` module
fn roundtrip<K, V, S>(store: &S, items: &BTreeMap<K, V>) -> Result<Vec<Option<V>>, S::Error>
where
    K: AsRef<[u8]>,
    V: ToBytes + FromBytes,
    S: Store<K, V>,
{
    store.put_many(items.iter())?;
    let result = store.get_many(items.keys());
    result
}

// should be moved to the `store` module
pub fn roundtrip_succeeds<K, V, S>(store: &S, items: BTreeMap<K, V>) -> Result<bool, S::Error>
where
    K: AsRef<[u8]>,
    V: ToBytes + FromBytes + Clone + PartialEq,
    S: Store<K, V>,
{
    let maybe_values: Vec<Option<V>> = roundtrip(store, &items)?;
    let values = match maybe_values.into_iter().collect::<Option<Vec<V>>>() {
        Some(values) => values,
        None => return Ok(false),
    };
    Ok(Iterator::eq(items.values(), values.iter()))
}
