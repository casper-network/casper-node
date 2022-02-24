use std::collections::BTreeMap;

use casper_types::bytesrepr::{FromBytes, ToBytes};

use crate::storage::{
    store::{Store, StoreExt},
    transaction_source::{Transaction, TransactionSource},
};

// should be moved to the `store` module
fn roundtrip<'a, K, V, X, S>(
    transaction_source: &'a X,
    store: &S,
    items: &BTreeMap<K, V>,
) -> Result<Vec<Option<V>>, S::Error>
where
    K: AsRef<[u8]>,
    V: ToBytes + FromBytes,
    X: TransactionSource<'a, Handle = S::Handle>,
    S: Store<K, V>,
    S::Error: From<X::Error>,
{
    let mut txn: X::ReadWriteTransaction = transaction_source.create_read_write_txn()?;
    store.put_many(&mut txn, items.iter())?;
    let result = store.get_many(&txn, items.keys());
    txn.commit()?;
    result
}

// should be moved to the `store` module
pub fn roundtrip_succeeds<'a, K, V, X, S>(
    transaction_source: &'a X,
    store: &S,
    items: BTreeMap<K, V>,
) -> Result<bool, S::Error>
where
    K: AsRef<[u8]>,
    V: ToBytes + FromBytes + Clone + PartialEq,
    X: TransactionSource<'a, Handle = S::Handle>,
    S: Store<K, V>,
    S::Error: From<X::Error>,
{
    let maybe_values: Vec<Option<V>> = roundtrip(transaction_source, store, &items)?;
    let values = match maybe_values.into_iter().collect::<Option<Vec<V>>>() {
        Some(values) => values,
        None => return Ok(false),
    };
    Ok(Iterator::eq(items.values(), values.iter()))
}
