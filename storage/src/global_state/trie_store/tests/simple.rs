use lmdb::DatabaseFlags;
use tempfile::tempdir;

use casper_types::bytesrepr::{self, Bytes, FromBytes, ToBytes};

use super::TestData;
use crate::global_state::{
    error,
    store::StoreExt,
    transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
    trie::Trie,
    trie_store::{lmdb::LmdbTrieStore, TrieStore},
    DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS,
};

fn put_succeeds<'a, K, V, S, X, E>(
    store: &S,
    transaction_source: &'a X,
    items: &[TestData<K, V>],
) -> Result<(), E>
where
    K: ToBytes,
    V: ToBytes,
    S: TrieStore<K, V>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error>,
{
    let mut txn: X::ReadWriteTransaction = transaction_source.create_read_write_txn()?;
    let items = items.iter().map(Into::into);
    store.put_many(&mut txn, items)?;
    txn.commit()?;
    Ok(())
}

#[test]
fn lmdb_put_succeeds() {
    let tmp_dir = tempdir().unwrap();
    let env = LmdbEnvironment::new(
        tmp_dir.path(),
        DEFAULT_MAX_DB_SIZE,
        DEFAULT_MAX_READERS,
        true,
    )
    .unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();
    let data = &super::create_data()[0..1];

    assert!(put_succeeds::<_, _, _, _, error::Error>(&store, &env, data).is_ok());

    tmp_dir.close().unwrap();
}

fn put_get_succeeds<'a, K, V, S, X, E>(
    store: &S,
    transaction_source: &'a X,
    items: &[TestData<K, V>],
) -> Result<Vec<Option<Trie<K, V>>>, E>
where
    K: ToBytes + FromBytes,
    V: ToBytes + FromBytes,
    S: TrieStore<K, V>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error>,
{
    let mut txn: X::ReadWriteTransaction = transaction_source.create_read_write_txn()?;
    let items = items.iter().map(Into::into);
    store.put_many(&mut txn, items.clone())?;
    let keys = items.map(|(k, _)| k);
    let ret = store.get_many(&txn, keys)?;
    txn.commit()?;
    Ok(ret)
}

#[test]
fn lmdb_put_get_succeeds() {
    let tmp_dir = tempdir().unwrap();
    let env = LmdbEnvironment::new(
        tmp_dir.path(),
        DEFAULT_MAX_DB_SIZE,
        DEFAULT_MAX_READERS,
        true,
    )
    .unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();
    let data = &super::create_data()[0..1];

    let expected: Vec<Trie<Bytes, Bytes>> = data.iter().cloned().map(|TestData(_, v)| v).collect();

    assert_eq!(
        expected,
        put_get_succeeds::<_, _, _, _, error::Error>(&store, &env, data)
            .expect("put_get_succeeds failed")
            .into_iter()
            .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
            .expect("one of the outputs was empty")
    );

    tmp_dir.close().unwrap();
}

#[test]
fn lmdb_put_get_many_succeeds() {
    let tmp_dir = tempdir().unwrap();
    let env = LmdbEnvironment::new(
        tmp_dir.path(),
        DEFAULT_MAX_DB_SIZE,
        DEFAULT_MAX_READERS,
        true,
    )
    .unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();
    let data = super::create_data();

    let expected: Vec<Trie<Bytes, Bytes>> = data.iter().cloned().map(|TestData(_, v)| v).collect();

    assert_eq!(
        expected,
        put_get_succeeds::<_, _, _, _, error::Error>(&store, &env, &data)
            .expect("put_get failed")
            .into_iter()
            .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
            .expect("one of the outputs was empty")
    );

    tmp_dir.close().unwrap();
}

fn uncommitted_read_write_txn_does_not_persist<'a, K, V, S, X, E>(
    store: &S,
    transaction_source: &'a X,
    items: &[TestData<K, V>],
) -> Result<Vec<Option<Trie<K, V>>>, E>
where
    K: ToBytes + FromBytes,
    V: ToBytes + FromBytes,
    S: TrieStore<K, V>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error>,
{
    {
        let mut txn: X::ReadWriteTransaction = transaction_source.create_read_write_txn()?;
        let items = items.iter().map(Into::into);
        store.put_many(&mut txn, items)?;
    }
    {
        let txn: X::ReadTransaction = transaction_source.create_read_txn()?;
        let keys = items.iter().map(|TestData(k, _)| k);
        let ret = store.get_many(&txn, keys)?;
        txn.commit()?;
        Ok(ret)
    }
}

#[test]
fn lmdb_uncommitted_read_write_txn_does_not_persist() {
    let tmp_dir = tempdir().unwrap();
    let env = LmdbEnvironment::new(
        tmp_dir.path(),
        DEFAULT_MAX_DB_SIZE,
        DEFAULT_MAX_READERS,
        true,
    )
    .unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();
    let data = super::create_data();

    assert_eq!(
        None,
        uncommitted_read_write_txn_does_not_persist::<_, _, _, _, error::Error>(
            &store, &env, &data,
        )
        .expect("uncommitted_read_write_txn_does_not_persist failed")
        .into_iter()
        .collect::<Option<Vec<Trie<Bytes, Bytes>>>>()
    );

    tmp_dir.close().unwrap();
}

fn read_write_transaction_does_not_block_read_transaction<'a, X, E>(
    transaction_source: &'a X,
) -> Result<(), E>
where
    X: TransactionSource<'a>,
    E: From<X::Error>,
{
    let read_write_txn = transaction_source.create_read_write_txn()?;
    let read_txn = transaction_source.create_read_txn()?;
    read_write_txn.commit()?;
    read_txn.commit()?;
    Ok(())
}

#[test]
fn lmdb_read_write_transaction_does_not_block_read_transaction() {
    let dir = tempdir().unwrap();
    let env =
        LmdbEnvironment::new(dir.path(), DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS, true).unwrap();

    assert!(read_write_transaction_does_not_block_read_transaction::<_, error::Error>(&env).is_ok())
}

fn reads_are_isolated<'a, S, X, E>(store: &S, env: &'a X) -> Result<(), E>
where
    S: TrieStore<Bytes, Bytes>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error> + From<bytesrepr::Error>,
{
    let TestData(leaf_1_hash, leaf_1) = &super::create_data()[0..1][0];

    {
        let read_txn_1 = env.create_read_txn()?;
        let result = store.get(&read_txn_1, leaf_1_hash)?;
        assert_eq!(result, None);

        {
            let mut write_txn = env.create_read_write_txn()?;
            store.put(&mut write_txn, leaf_1_hash, leaf_1)?;
            write_txn.commit()?;
        }

        let result = store.get(&read_txn_1, leaf_1_hash)?;
        read_txn_1.commit()?;
        assert_eq!(result, None);
    }

    {
        let read_txn_2 = env.create_read_txn()?;
        let result = store.get(&read_txn_2, leaf_1_hash)?;
        read_txn_2.commit()?;
        assert_eq!(result, Some(leaf_1.to_owned()));
    }

    Ok(())
}

#[test]
fn lmdb_reads_are_isolated() {
    let dir = tempdir().unwrap();
    let env =
        LmdbEnvironment::new(dir.path(), DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS, true).unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();

    assert!(reads_are_isolated::<_, _, error::Error>(&store, &env).is_ok())
}

fn reads_are_isolated_2<'a, S, X, E>(store: &S, env: &'a X) -> Result<(), E>
where
    S: TrieStore<Bytes, Bytes>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error> + From<bytesrepr::Error>,
{
    let data = super::create_data();
    let TestData(ref leaf_1_hash, ref leaf_1) = data[0];
    let TestData(ref leaf_2_hash, ref leaf_2) = data[1];

    {
        let mut write_txn = env.create_read_write_txn()?;
        store.put(&mut write_txn, leaf_1_hash, leaf_1)?;
        write_txn.commit()?;
    }

    {
        let read_txn_1 = env.create_read_txn()?;
        {
            let mut write_txn = env.create_read_write_txn()?;
            store.put(&mut write_txn, leaf_2_hash, leaf_2)?;
            write_txn.commit()?;
        }
        let result = store.get(&read_txn_1, leaf_1_hash)?;
        read_txn_1.commit()?;
        assert_eq!(result, Some(leaf_1.to_owned()));
    }

    {
        let read_txn_2 = env.create_read_txn()?;
        let result = store.get(&read_txn_2, leaf_2_hash)?;
        read_txn_2.commit()?;
        assert_eq!(result, Some(leaf_2.to_owned()));
    }

    Ok(())
}

#[test]
fn lmdb_reads_are_isolated_2() {
    let dir = tempdir().unwrap();
    let env =
        LmdbEnvironment::new(dir.path(), DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS, true).unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();

    assert!(reads_are_isolated_2::<_, _, error::Error>(&store, &env).is_ok())
}

fn dbs_are_isolated<'a, S, X, E>(env: &'a X, store_a: &S, store_b: &S) -> Result<(), E>
where
    S: TrieStore<Bytes, Bytes>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error> + From<bytesrepr::Error>,
{
    let data = super::create_data();
    let TestData(ref leaf_1_hash, ref leaf_1) = data[0];
    let TestData(ref leaf_2_hash, ref leaf_2) = data[1];

    {
        let mut write_txn = env.create_read_write_txn()?;
        store_a.put(&mut write_txn, leaf_1_hash, leaf_1)?;
        write_txn.commit()?;
    }

    {
        let mut write_txn = env.create_read_write_txn()?;
        store_b.put(&mut write_txn, leaf_2_hash, leaf_2)?;
        write_txn.commit()?;
    }

    {
        let read_txn = env.create_read_txn()?;
        let result = store_a.get(&read_txn, leaf_1_hash)?;
        assert_eq!(result, Some(leaf_1.to_owned()));
        let result = store_a.get(&read_txn, leaf_2_hash)?;
        assert_eq!(result, None);
        read_txn.commit()?;
    }

    {
        let read_txn = env.create_read_txn()?;
        let result = store_b.get(&read_txn, leaf_1_hash)?;
        assert_eq!(result, None);
        let result = store_b.get(&read_txn, leaf_2_hash)?;
        assert_eq!(result, Some(leaf_2.to_owned()));
        read_txn.commit()?;
    }

    Ok(())
}

#[test]
fn lmdb_dbs_are_isolated() {
    let dir = tempdir().unwrap();
    let env =
        LmdbEnvironment::new(dir.path(), DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS, true).unwrap();
    let store_a = LmdbTrieStore::new(&env, Some("a"), DatabaseFlags::empty()).unwrap();
    let store_b = LmdbTrieStore::new(&env, Some("b"), DatabaseFlags::empty()).unwrap();

    assert!(dbs_are_isolated::<_, _, error::Error>(&env, &store_a, &store_b).is_ok())
}

fn transactions_can_be_used_across_sub_databases<'a, S, X, E>(
    env: &'a X,
    store_a: &S,
    store_b: &S,
) -> Result<(), E>
where
    S: TrieStore<Bytes, Bytes>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error> + From<bytesrepr::Error>,
{
    let data = super::create_data();
    let TestData(ref leaf_1_hash, ref leaf_1) = data[0];
    let TestData(ref leaf_2_hash, ref leaf_2) = data[1];

    {
        let mut write_txn = env.create_read_write_txn()?;
        store_a.put(&mut write_txn, leaf_1_hash, leaf_1)?;
        store_b.put(&mut write_txn, leaf_2_hash, leaf_2)?;
        write_txn.commit()?;
    }

    {
        let read_txn = env.create_read_txn()?;
        let result = store_a.get(&read_txn, leaf_1_hash)?;
        assert_eq!(result, Some(leaf_1.to_owned()));
        let result = store_b.get(&read_txn, leaf_2_hash)?;
        assert_eq!(result, Some(leaf_2.to_owned()));
        read_txn.commit()?;
    }

    Ok(())
}

#[test]
fn lmdb_transactions_can_be_used_across_sub_databases() {
    let dir = tempdir().unwrap();
    let env =
        LmdbEnvironment::new(dir.path(), DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS, true).unwrap();
    let store_a = LmdbTrieStore::new(&env, Some("a"), DatabaseFlags::empty()).unwrap();
    let store_b = LmdbTrieStore::new(&env, Some("b"), DatabaseFlags::empty()).unwrap();

    assert!(
        transactions_can_be_used_across_sub_databases::<_, _, error::Error>(
            &env, &store_a, &store_b,
        )
        .is_ok()
    )
}

fn uncommitted_transactions_across_sub_databases_do_not_persist<'a, S, X, E>(
    env: &'a X,
    store_a: &S,
    store_b: &S,
) -> Result<(), E>
where
    S: TrieStore<Bytes, Bytes>,
    X: TransactionSource<'a, Handle = S::Handle>,
    S::Error: From<X::Error>,
    E: From<S::Error> + From<X::Error> + From<bytesrepr::Error>,
{
    let data = super::create_data();
    let TestData(ref leaf_1_hash, ref leaf_1) = data[0];
    let TestData(ref leaf_2_hash, ref leaf_2) = data[1];

    {
        let mut write_txn = env.create_read_write_txn()?;
        store_a.put(&mut write_txn, leaf_1_hash, leaf_1)?;
        store_b.put(&mut write_txn, leaf_2_hash, leaf_2)?;
    }

    {
        let read_txn = env.create_read_txn()?;
        let result = store_a.get(&read_txn, leaf_1_hash)?;
        assert_eq!(result, None);
        let result = store_b.get(&read_txn, leaf_2_hash)?;
        assert_eq!(result, None);
        read_txn.commit()?;
    }

    Ok(())
}

#[test]
fn lmdb_uncommitted_transactions_across_sub_databases_do_not_persist() {
    let dir = tempdir().unwrap();
    let env =
        LmdbEnvironment::new(dir.path(), DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS, true).unwrap();
    let store_a = LmdbTrieStore::new(&env, Some("a"), DatabaseFlags::empty()).unwrap();
    let store_b = LmdbTrieStore::new(&env, Some("b"), DatabaseFlags::empty()).unwrap();

    assert!(
        uncommitted_transactions_across_sub_databases_do_not_persist::<_, _, error::Error>(
            &env, &store_a, &store_b,
        )
        .is_ok()
    )
}
