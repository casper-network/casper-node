use tempfile::tempdir;

use casper_types::bytesrepr::{Bytes, FromBytes, ToBytes};

use super::TestData;
use crate::storage::{
    error::{self, in_memory},
    store::StoreExt,
    trie::Trie,
    trie_store::{db::RocksDbStore, in_memory::InMemoryTrieStore, TrieStore},
};

fn put_succeeds<K, V, S, E>(store: &S, items: &[TestData<K, V>]) -> Result<(), E>
where
    K: ToBytes,
    V: ToBytes,
    S: TrieStore<K, V>,
    E: From<S::Error>,
{
    let items = items.iter().map(Into::into);
    store.put_many(items)?;
    Ok(())
}

#[test]
fn in_memory_put_succeeds() {
    let store = InMemoryTrieStore::new();
    let data = &super::create_data()[0..1];
    assert!(put_succeeds::<_, _, _, in_memory::Error>(&store, data).is_ok());
}

#[test]
fn rocksdb_put_succeeds() {
    let tmp_dir = tempdir().unwrap();
    let store = RocksDbStore::new(tmp_dir.path()).unwrap();
    let data = &super::create_data()[0..1];
    assert!(put_succeeds::<_, _, _, error::Error>(&store, data).is_ok());
    tmp_dir.close().unwrap();
}

fn put_get_succeeds<K, V, S, E>(
    store: &S,
    items: &[TestData<K, V>],
) -> Result<Vec<Option<Trie<K, V>>>, E>
where
    K: ToBytes + FromBytes,
    V: ToBytes + FromBytes,
    S: TrieStore<K, V>,
    E: From<S::Error>,
{
    let items = items.iter().map(Into::into);
    store.put_many(items.clone())?;
    let keys = items.map(|(k, _)| k);
    let ret = store.get_many(keys)?;
    Ok(ret)
}

#[test]
fn in_memory_put_get_succeeds() {
    let store = InMemoryTrieStore::new();
    let data = &super::create_data()[0..1];

    let expected: Vec<Trie<Bytes, Bytes>> = data.iter().cloned().map(|TestData(_, v)| v).collect();

    assert_eq!(
        expected,
        put_get_succeeds::<_, _, _, in_memory::Error>(&store, data)
            .expect("put_get_succeeds failed")
            .into_iter()
            .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
            .expect("one of the outputs was empty")
    )
}

#[test]
fn rocksdb_put_get_succeeds() {
    let tmp_dir = tempdir().unwrap();
    let store = RocksDbStore::new(tmp_dir.path()).unwrap();
    let data = &super::create_data()[0..1];

    let expected: Vec<Trie<Bytes, Bytes>> = data.iter().cloned().map(|TestData(_, v)| v).collect();

    assert_eq!(
        expected,
        put_get_succeeds::<_, _, _, error::Error>(&store, data)
            .expect("put_get_succeeds failed")
            .into_iter()
            .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
            .expect("one of the outputs was empty")
    );

    tmp_dir.close().unwrap();
}

#[test]
fn in_memory_put_get_many_succeeds() {
    let store = InMemoryTrieStore::new();
    let data = super::create_data();

    let expected: Vec<Trie<Bytes, Bytes>> = data.iter().cloned().map(|TestData(_, v)| v).collect();

    assert_eq!(
        expected,
        put_get_succeeds::<_, _, _, error::Error>(&store, &data)
            .expect("put_get failed")
            .into_iter()
            .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
            .expect("one of the outputs was empty")
    )
}

#[test]
fn rocksdb_put_get_many_succeeds() {
    let tmp_dir = tempdir().unwrap();
    let store = RocksDbStore::new(tmp_dir.path()).unwrap();
    let data = super::create_data();

    let expected: Vec<Trie<Bytes, Bytes>> = data.iter().cloned().map(|TestData(_, v)| v).collect();

    assert_eq!(
        expected,
        put_get_succeeds::<_, _, _, error::Error>(&store, &data)
            .expect("put_get failed")
            .into_iter()
            .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
            .expect("one of the outputs was empty")
    );

    tmp_dir.close().unwrap();
}
