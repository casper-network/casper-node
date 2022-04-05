mod delete;
mod ee_699;
mod keys;
mod proptests;
mod read;
mod scan;
mod synchronize;
mod write;

use std::{collections::HashMap, convert, ops::Not};

use lmdb::DatabaseFlags;
use tempfile::{tempdir, TempDir};

use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::{
    shared::newtypes::CorrelationId,
    storage::{
        error::{self, in_memory},
        transaction_source::{
            in_memory::InMemoryEnvironment, lmdb::LmdbEnvironment, Readable, Transaction,
            TransactionSource,
        },
        trie::{merkle_proof::TrieMerkleProof, Pointer, Trie},
        trie_store::{
            self,
            in_memory::InMemoryTrieStore,
            lmdb::LmdbTrieStore,
            operations::{self, read, read_with_proof, write, ReadResult, WriteResult},
            TrieStore,
        },
        DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS,
    },
};

const TEST_KEY_LENGTH: usize = 7;

/// A short key type for tests.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TestKey([u8; TEST_KEY_LENGTH]);

impl ToBytes for TestKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(self.0.to_vec())
    }

    fn serialized_length(&self) -> usize {
        TEST_KEY_LENGTH
    }
}

impl FromBytes for TestKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, rem) = bytes.split_at(TEST_KEY_LENGTH);
        let mut ret = [0u8; TEST_KEY_LENGTH];
        ret.copy_from_slice(key);
        Ok((TestKey(ret), rem))
    }
}

const TEST_VAL_LENGTH: usize = 6;

/// A short value type for tests.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct TestValue([u8; TEST_VAL_LENGTH]);

impl ToBytes for TestValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(self.0.to_vec())
    }

    fn serialized_length(&self) -> usize {
        TEST_VAL_LENGTH
    }
}

impl FromBytes for TestValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, rem) = bytes.split_at(TEST_VAL_LENGTH);
        let mut ret = [0u8; TEST_VAL_LENGTH];
        ret.copy_from_slice(key);
        Ok((TestValue(ret), rem))
    }
}

type TestTrie = Trie<TestKey, TestValue>;

type HashedTestTrie = HashedTrie<TestKey, TestValue>;

/// A pairing of a trie element and its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HashedTrie<K, V> {
    hash: Digest,
    trie: Trie<K, V>,
}

impl<K: ToBytes, V: ToBytes> HashedTrie<K, V> {
    pub fn new(trie: Trie<K, V>) -> Result<Self, bytesrepr::Error> {
        let trie_bytes = trie.to_bytes()?;
        let hash = Digest::hash(&trie_bytes);
        Ok(HashedTrie { hash, trie })
    }
}

const EMPTY_HASHED_TEST_TRIES: &[HashedTestTrie] = &[];

const TEST_LEAVES_LENGTH: usize = 6;

/// Keys have been chosen deliberately and the `create_` functions below depend
/// on these exact definitions.  Values are arbitrary.
const TEST_LEAVES: [TestTrie; TEST_LEAVES_LENGTH] = [
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"value0"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 1]),
        value: TestValue(*b"value1"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 2, 0, 0, 0]),
        value: TestValue(*b"value2"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 255, 0]),
        value: TestValue(*b"value3"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 1, 0, 0, 0, 0, 0]),
        value: TestValue(*b"value4"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 2, 0, 0, 0, 0]),
        value: TestValue(*b"value5"),
    },
];

const TEST_LEAVES_UPDATED: [TestTrie; TEST_LEAVES_LENGTH] = [
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueA"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 1]),
        value: TestValue(*b"valueB"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 2, 0, 0, 0]),
        value: TestValue(*b"valueC"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 255, 0]),
        value: TestValue(*b"valueD"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 1, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueE"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 2, 0, 0, 0, 0]),
        value: TestValue(*b"valueF"),
    },
];

const TEST_LEAVES_NON_COLLIDING: [TestTrie; TEST_LEAVES_LENGTH] = [
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueA"),
    },
    Trie::Leaf {
        key: TestKey([1u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueB"),
    },
    Trie::Leaf {
        key: TestKey([2u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueC"),
    },
    Trie::Leaf {
        key: TestKey([3u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueD"),
    },
    Trie::Leaf {
        key: TestKey([4u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueE"),
    },
    Trie::Leaf {
        key: TestKey([5u8, 0, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueF"),
    },
];

const TEST_LEAVES_ADJACENTS: [TestTrie; TEST_LEAVES_LENGTH] = [
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 2]),
        value: TestValue(*b"valueA"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 0, 3]),
        value: TestValue(*b"valueB"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 3, 0, 0, 0]),
        value: TestValue(*b"valueC"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 0, 0, 0, 1, 0]),
        value: TestValue(*b"valueD"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 2, 0, 0, 0, 0, 0]),
        value: TestValue(*b"valueE"),
    },
    Trie::Leaf {
        key: TestKey([0u8, 0, 3, 0, 0, 0, 0]),
        value: TestValue(*b"valueF"),
    },
];

type TrieGenerator<K, V> = fn() -> Result<(Digest, Vec<HashedTrie<K, V>>), bytesrepr::Error>;

const TEST_TRIE_GENERATORS_LENGTH: usize = 7;

const TEST_TRIE_GENERATORS: [TrieGenerator<TestKey, TestValue>; TEST_TRIE_GENERATORS_LENGTH] = [
    create_0_leaf_trie,
    create_1_leaf_trie,
    create_2_leaf_trie,
    create_3_leaf_trie,
    create_4_leaf_trie,
    create_5_leaf_trie,
    create_6_leaf_trie,
];

fn hash_test_tries(tries: &[TestTrie]) -> Result<Vec<HashedTestTrie>, bytesrepr::Error> {
    tries
        .iter()
        .map(|trie| HashedTestTrie::new(trie.to_owned()))
        .collect()
}

fn create_0_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let root = HashedTrie::new(Trie::node(&[]))?;

    let root_hash: Digest = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn create_1_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let leaves = hash_test_tries(&TEST_LEAVES[..1])?;

    let root = HashedTrie::new(Trie::node(&[(0, Pointer::LeafPointer(leaves[0].hash))]))?;

    let root_hash: Digest = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(leaves);
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn create_2_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let leaves = hash_test_tries(&TEST_LEAVES[..2])?;

    let node = HashedTrie::new(Trie::node(&[
        (0, Pointer::LeafPointer(leaves[0].hash)),
        (1, Pointer::LeafPointer(leaves[1].hash)),
    ]))?;

    let ext = HashedTrie::new(Trie::extension(
        vec![0u8, 0, 0, 0, 0],
        Pointer::NodePointer(node.hash),
    ))?;

    let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(ext.hash))]))?;

    let root_hash = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root, ext, node];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(leaves);
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn create_3_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let leaves = hash_test_tries(&TEST_LEAVES[..3])?;

    let node_1 = HashedTrie::new(Trie::node(&[
        (0, Pointer::LeafPointer(leaves[0].hash)),
        (1, Pointer::LeafPointer(leaves[1].hash)),
    ]))?;

    let ext_1 = HashedTrie::new(Trie::extension(
        vec![0u8, 0],
        Pointer::NodePointer(node_1.hash),
    ))?;

    let node_2 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(ext_1.hash)),
        (2, Pointer::LeafPointer(leaves[2].hash)),
    ]))?;

    let ext_2 = HashedTrie::new(Trie::extension(
        vec![0u8, 0],
        Pointer::NodePointer(node_2.hash),
    ))?;

    let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(ext_2.hash))]))?;

    let root_hash = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root, ext_2, node_2, ext_1, node_1];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(leaves);
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn create_4_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let leaves = hash_test_tries(&TEST_LEAVES[..4])?;

    let node_1 = HashedTrie::new(Trie::node(&[
        (0, Pointer::LeafPointer(leaves[0].hash)),
        (1, Pointer::LeafPointer(leaves[1].hash)),
    ]))?;

    let node_2 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(node_1.hash)),
        (255, Pointer::LeafPointer(leaves[3].hash)),
    ]))?;

    let ext_1 = HashedTrie::new(Trie::extension(
        vec![0u8],
        Pointer::NodePointer(node_2.hash),
    ))?;

    let node_3 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(ext_1.hash)),
        (2, Pointer::LeafPointer(leaves[2].hash)),
    ]))?;

    let ext_2 = HashedTrie::new(Trie::extension(
        vec![0u8, 0],
        Pointer::NodePointer(node_3.hash),
    ))?;

    let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(ext_2.hash))]))?;

    let root_hash = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root, ext_2, node_3, ext_1, node_2, node_1];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(leaves);
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn create_5_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let leaves = hash_test_tries(&TEST_LEAVES[..5])?;

    let node_1 = HashedTrie::new(Trie::node(&[
        (0, Pointer::LeafPointer(leaves[0].hash)),
        (1, Pointer::LeafPointer(leaves[1].hash)),
    ]))?;

    let node_2 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(node_1.hash)),
        (255, Pointer::LeafPointer(leaves[3].hash)),
    ]))?;

    let ext_1 = HashedTrie::new(Trie::extension(
        vec![0u8],
        Pointer::NodePointer(node_2.hash),
    ))?;

    let node_3 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(ext_1.hash)),
        (2, Pointer::LeafPointer(leaves[2].hash)),
    ]))?;

    let ext_2 = HashedTrie::new(Trie::extension(
        vec![0u8],
        Pointer::NodePointer(node_3.hash),
    ))?;

    let node_4 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(ext_2.hash)),
        (1, Pointer::LeafPointer(leaves[4].hash)),
    ]))?;

    let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(node_4.hash))]))?;

    let root_hash = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root, node_4, ext_2, node_3, ext_1, node_2, node_1];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(leaves);
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn create_6_leaf_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
    let leaves = hash_test_tries(&TEST_LEAVES)?;

    let node_1 = HashedTrie::new(Trie::node(&[
        (0, Pointer::LeafPointer(leaves[0].hash)),
        (1, Pointer::LeafPointer(leaves[1].hash)),
    ]))?;

    let node_2 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(node_1.hash)),
        (255, Pointer::LeafPointer(leaves[3].hash)),
    ]))?;

    let ext = HashedTrie::new(Trie::extension(
        vec![0u8],
        Pointer::NodePointer(node_2.hash),
    ))?;

    let node_3 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(ext.hash)),
        (2, Pointer::LeafPointer(leaves[2].hash)),
    ]))?;

    let node_4 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(node_3.hash)),
        (2, Pointer::LeafPointer(leaves[5].hash)),
    ]))?;

    let node_5 = HashedTrie::new(Trie::node(&[
        (0, Pointer::NodePointer(node_4.hash)),
        (1, Pointer::LeafPointer(leaves[4].hash)),
    ]))?;

    let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(node_5.hash))]))?;

    let root_hash = root.hash;

    let parents: Vec<HashedTestTrie> = vec![root, node_5, node_4, node_3, ext, node_2, node_1];

    let tries: Vec<HashedTestTrie> = {
        let mut ret = Vec::new();
        ret.extend(leaves);
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

fn put_tries<'a, K, V, R, S, E>(
    environment: &'a R,
    store: &S,
    tries: &[HashedTrie<K, V>],
) -> Result<(), E>
where
    K: ToBytes,
    V: ToBytes,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    if tries.is_empty() {
        return Ok(());
    }
    let mut txn = environment.create_read_write_txn()?;
    for HashedTrie { hash, trie } in tries.iter() {
        store.put(&mut txn, hash, trie)?;
    }
    txn.commit()?;
    Ok(())
}

// A context for holding lmdb-based test resources
struct LmdbTestContext {
    _temp_dir: TempDir,
    environment: LmdbEnvironment,
    store: LmdbTrieStore,
}

impl LmdbTestContext {
    fn new<K, V>(tries: &[HashedTrie<K, V>]) -> anyhow::Result<Self>
    where
        K: FromBytes + ToBytes,
        V: FromBytes + ToBytes,
    {
        let _temp_dir = tempdir()?;
        let environment = LmdbEnvironment::new(
            &_temp_dir.path(),
            DEFAULT_TEST_MAX_DB_SIZE,
            DEFAULT_TEST_MAX_READERS,
            true,
        )?;
        let store = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())?;
        put_tries::<_, _, _, _, error::Error>(&environment, &store, tries)?;
        Ok(LmdbTestContext {
            _temp_dir,
            environment,
            store,
        })
    }

    fn update<K, V>(&self, tries: &[HashedTrie<K, V>]) -> anyhow::Result<()>
    where
        K: ToBytes,
        V: ToBytes,
    {
        put_tries::<_, _, _, _, error::Error>(&self.environment, &self.store, tries)?;
        Ok(())
    }
}

// A context for holding in-memory test resources
struct InMemoryTestContext {
    environment: InMemoryEnvironment,
    store: InMemoryTrieStore,
}

impl InMemoryTestContext {
    fn new<K, V>(tries: &[HashedTrie<K, V>]) -> anyhow::Result<Self>
    where
        K: ToBytes,
        V: ToBytes,
    {
        let environment = InMemoryEnvironment::new();
        let store = InMemoryTrieStore::new(&environment, None);
        put_tries::<_, _, _, _, in_memory::Error>(&environment, &store, tries)?;
        Ok(InMemoryTestContext { environment, store })
    }

    fn update<K, V>(&self, tries: &[HashedTrie<K, V>]) -> anyhow::Result<()>
    where
        K: ToBytes,
        V: ToBytes,
    {
        put_tries::<_, _, _, _, in_memory::Error>(&self.environment, &self.store, tries)?;
        Ok(())
    }
}

fn check_leaves_exist<K, V, T, S, E>(
    correlation_id: CorrelationId,
    txn: &T,
    store: &S,
    root: &Digest,
    leaves: &[Trie<K, V>],
) -> Result<Vec<bool>, E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug,
    V: ToBytes + FromBytes + Eq + Copy,
    T: Readable<Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let mut ret = Vec::new();

    for leaf in leaves {
        if let Trie::Leaf { key, value } = leaf {
            let maybe_value: ReadResult<V> =
                read::<_, _, _, _, E>(correlation_id, txn, store, root, key)?;
            ret.push(ReadResult::Found(*value) == maybe_value)
        } else {
            panic!("leaves should only contain leaves")
        }
    }
    Ok(ret)
}

/// For a given vector of leaves check the merkle proofs exist and are correct
fn check_merkle_proofs<K, V, T, S, E>(
    correlation_id: CorrelationId,
    txn: &T,
    store: &S,
    root: &Digest,
    leaves: &[Trie<K, V>],
) -> Result<Vec<bool>, E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    V: ToBytes + FromBytes + Eq + Copy,
    T: Readable<Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let mut ret = Vec::new();

    for leaf in leaves {
        if let Trie::Leaf { key, value } = leaf {
            let maybe_proof: ReadResult<TrieMerkleProof<K, V>> =
                read_with_proof::<_, _, _, _, E>(correlation_id, txn, store, root, key)?;
            match maybe_proof {
                ReadResult::Found(proof) => {
                    let hash = proof.compute_state_hash()?;
                    ret.push(hash == *root && proof.value() == value);
                }
                ReadResult::NotFound => {
                    ret.push(false);
                }
                ReadResult::RootNotFound => panic!("Root not found!"),
            };
        } else {
            panic!("leaves should only contain leaves")
        }
    }
    Ok(ret)
}

fn check_keys<K, V, T, S, E>(
    correlation_id: CorrelationId,
    txn: &T,
    store: &S,
    root: &Digest,
    leaves: &[Trie<K, V>],
) -> bool
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Clone + Ord,
    V: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    T: Readable<Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let expected = {
        let mut tmp = leaves
            .iter()
            .filter_map(Trie::key)
            .cloned()
            .collect::<Vec<K>>();
        tmp.sort();
        tmp
    };
    let actual = {
        let mut tmp = operations::keys::<_, _, _, _>(correlation_id, txn, store, root)
            .filter_map(Result::ok)
            .collect::<Vec<K>>();
        tmp.sort();
        tmp
    };
    expected == actual
}

fn check_leaves<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root: &Digest,
    present: &[Trie<K, V>],
    absent: &[Trie<K, V>],
) -> Result<(), E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy + Clone + Ord,
    V: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let txn: R::ReadTransaction = environment.create_read_txn()?;

    assert!(
        check_leaves_exist::<_, _, _, _, E>(correlation_id, &txn, store, root, present)?
            .into_iter()
            .all(convert::identity)
    );

    assert!(
        check_merkle_proofs::<_, _, _, _, E>(correlation_id, &txn, store, root, present)?
            .into_iter()
            .all(convert::identity)
    );

    assert!(
        check_leaves_exist::<_, _, _, _, E>(correlation_id, &txn, store, root, absent)?
            .into_iter()
            .all(bool::not)
    );

    assert!(
        check_merkle_proofs::<_, _, _, _, E>(correlation_id, &txn, store, root, absent)?
            .into_iter()
            .all(bool::not)
    );

    assert!(check_keys::<_, _, _, _, E>(
        correlation_id,
        &txn,
        store,
        root,
        present,
    ));

    txn.commit()?;
    Ok(())
}

fn write_leaves<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root_hash: &Digest,
    leaves: &[Trie<K, V>],
) -> Result<Vec<WriteResult>, E>
where
    K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
    V: ToBytes + FromBytes + Clone + Eq,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let mut results = Vec::new();
    if leaves.is_empty() {
        return Ok(results);
    }
    let mut root_hash = root_hash.to_owned();
    let mut txn = environment.create_read_write_txn()?;

    for leaf in leaves.iter() {
        if let Trie::Leaf { key, value } = leaf {
            let write_result =
                write::<_, _, _, _, E>(correlation_id, &mut txn, store, &root_hash, key, value)?;
            match write_result {
                WriteResult::Written(hash) => {
                    root_hash = hash;
                }
                WriteResult::AlreadyExists => (),
                WriteResult::RootNotFound => panic!("write_leaves given an invalid root"),
            };
            results.push(write_result);
        } else {
            panic!("leaves should contain only leaves");
        }
    }
    txn.commit()?;
    Ok(results)
}

fn check_pairs_proofs<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root_hashes: &[Digest],
    pairs: &[(K, V)],
) -> Result<bool, E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy + Clone + Ord,
    V: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let txn = environment.create_read_txn()?;
    for (index, root_hash) in root_hashes.iter().enumerate() {
        for (key, value) in &pairs[..=index] {
            let maybe_proof =
                read_with_proof::<_, _, _, _, E>(correlation_id, &txn, store, root_hash, key)?;
            match maybe_proof {
                ReadResult::Found(proof) => {
                    let hash = proof.compute_state_hash()?;
                    if hash != *root_hash || proof.value() != value {
                        return Ok(false);
                    }
                }
                ReadResult::NotFound => return Ok(false),
                ReadResult::RootNotFound => panic!("Root not found!"),
            };
        }
    }
    Ok(true)
}

fn check_pairs<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root_hashes: &[Digest],
    pairs: &[(K, V)],
) -> Result<bool, E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Clone + Ord,
    V: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let txn = environment.create_read_txn()?;
    for (index, root_hash) in root_hashes.iter().enumerate() {
        for (key, value) in &pairs[..=index] {
            let result = read::<_, _, _, _, E>(correlation_id, &txn, store, root_hash, key)?;
            if ReadResult::Found(*value) != result {
                return Ok(false);
            }
        }
        let expected = {
            let mut tmp = pairs[..=index]
                .iter()
                .map(|(k, _)| k)
                .cloned()
                .collect::<Vec<K>>();
            tmp.sort();
            tmp
        };
        let actual = {
            let mut tmp = operations::keys::<_, _, _, _>(correlation_id, &txn, store, root_hash)
                .filter_map(Result::ok)
                .collect::<Vec<K>>();
            tmp.sort();
            tmp
        };
        if expected != actual {
            return Ok(false);
        }
    }
    Ok(true)
}

fn write_pairs<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root_hash: &Digest,
    pairs: &[(K, V)],
) -> Result<Vec<Digest>, E>
where
    K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
    V: ToBytes + FromBytes + Clone + Eq,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let mut results = Vec::new();
    if pairs.is_empty() {
        return Ok(results);
    }
    let mut root_hash = root_hash.to_owned();
    let mut txn = environment.create_read_write_txn()?;

    for (key, value) in pairs.iter() {
        match write::<_, _, _, _, E>(correlation_id, &mut txn, store, &root_hash, key, value)? {
            WriteResult::Written(hash) => {
                root_hash = hash;
            }
            WriteResult::AlreadyExists => (),
            WriteResult::RootNotFound => panic!("write_leaves given an invalid root"),
        };
        results.push(root_hash);
    }
    txn.commit()?;
    Ok(results)
}

fn writes_to_n_leaf_empty_trie_had_expected_results<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    states: &[Digest],
    test_leaves: &[Trie<K, V>],
) -> Result<Vec<Digest>, E>
where
    K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug + Copy + Ord,
    V: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug + Copy,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let mut states = states.to_vec();

    // Write set of leaves to the trie
    let hashes = write_leaves::<_, _, _, _, E>(
        correlation_id,
        environment,
        store,
        states.last().unwrap(),
        test_leaves,
    )?
    .into_iter()
    .map(|result| match result {
        WriteResult::Written(root_hash) => root_hash,
        _ => panic!("write_leaves resulted in non-write"),
    })
    .collect::<Vec<Digest>>();

    states.extend(hashes);

    // Check that the expected set of leaves is in the trie at every
    // state, and that the set of other leaves is not.
    for (num_leaves, state) in states.iter().enumerate() {
        let (used, unused) = test_leaves.split_at(num_leaves);
        check_leaves::<_, _, _, _, E>(correlation_id, environment, store, state, used, unused)?;
    }

    Ok(states)
}

impl InMemoryEnvironment {
    pub fn dump<K, V>(
        &self,
        maybe_name: Option<&str>,
    ) -> Result<HashMap<Digest, Trie<K, V>>, in_memory::Error>
    where
        K: FromBytes,
        V: FromBytes,
    {
        let name = maybe_name
            .map(|name| format!("{}-{}", trie_store::NAME, name))
            .unwrap_or_else(|| trie_store::NAME.to_string());
        let data = self.data(Some(&name))?.unwrap();
        data.into_iter()
            .map(|(hash_bytes, trie_bytes)| {
                let hash: Digest = bytesrepr::deserialize_from_slice(hash_bytes)?;
                let trie: Trie<K, V> = bytesrepr::deserialize_from_slice(trie_bytes)?;
                Ok((hash, trie))
            })
            .collect::<Result<HashMap<Digest, Trie<K, V>>, bytesrepr::Error>>()
            .map_err(Into::into)
    }
}
