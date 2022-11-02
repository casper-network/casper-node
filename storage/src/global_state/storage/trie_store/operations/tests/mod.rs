mod delete;
mod ee_699;
mod keys;
mod read;
mod scan;
mod synchronize;
mod write;

use std::{convert, ops::Not};

use lmdb::DatabaseFlags;
use once_cell::sync::Lazy;
use tempfile::{tempdir, TempDir};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{self, ToBytes},
    Key, StoredValue,
};

use crate::global_state::{
    shared::CorrelationId,
    storage::{
        error,
        transaction_source::{lmdb::LmdbEnvironment, Readable, Transaction, TransactionSource},
        trie::{merkle_proof::TrieMerkleProof, Pointer, Trie},
        trie_store::{
            lmdb::LmdbTrieStore,
            operations::{
                self, read, read_with_proof, test_key, test_value, write, ReadResult, WriteResult,
            },
            TrieStore,
        },
        DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS,
    },
};

type TestTrie = Trie;

type HashedTestTrie = HashedTrie;

/// A pairing of a trie element and its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HashedTrie {
    hash: Digest,
    trie: Trie,
}

impl HashedTrie {
    pub fn new(trie: Trie) -> Result<Self, bytesrepr::Error> {
        let trie_bytes = trie.to_bytes()?;
        let hash = Digest::hash(&trie_bytes);
        Ok(HashedTrie { hash, trie })
    }
}

const EMPTY_HASHED_TEST_TRIES: &[HashedTestTrie] = &[];

const TEST_LEAVES_LENGTH: usize = 6;

/// Keys have been chosen deliberately and the `create_` functions below depend
/// on these exact definitions.  Values are arbitrary.
const TEST_LEAVES: Lazy<[TestTrie; TEST_LEAVES_LENGTH]> = Lazy::new(|| {
    [
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"value0"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 1]),
            value: test_value(*b"value1"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 2, 0, 0, 0]),
            value: test_value(*b"value2"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 255, 0]),
            value: test_value(*b"value3"),
        },
        Trie::Leaf {
            key: test_key([0u8, 1, 0, 0, 0, 0, 0]),
            value: test_value(*b"value4"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 2, 0, 0, 0, 0]),
            value: test_value(*b"value5"),
        },
    ]
});

const TEST_LEAVES_UPDATED: Lazy<[TestTrie; TEST_LEAVES_LENGTH]> = Lazy::new(|| {
    [
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueA"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 1]),
            value: test_value(*b"valueB"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 2, 0, 0, 0]),
            value: test_value(*b"valueC"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 255, 0]),
            value: test_value(*b"valueD"),
        },
        Trie::Leaf {
            key: test_key([0u8, 1, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueE"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 2, 0, 0, 0, 0]),
            value: test_value(*b"valueF"),
        },
    ]
});

const TEST_LEAVES_NON_COLLIDING: Lazy<[TestTrie; TEST_LEAVES_LENGTH]> = Lazy::new(|| {
    [
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueA"),
        },
        Trie::Leaf {
            key: test_key([1u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueB"),
        },
        Trie::Leaf {
            key: test_key([2u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueC"),
        },
        Trie::Leaf {
            key: test_key([3u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueD"),
        },
        Trie::Leaf {
            key: test_key([4u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueE"),
        },
        Trie::Leaf {
            key: test_key([5u8, 0, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueF"),
        },
    ]
});

const TEST_LEAVES_ADJACENTS: Lazy<[TestTrie; TEST_LEAVES_LENGTH]> = Lazy::new(|| {
    [
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 2]),
            value: test_value(*b"valueA"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 0, 3]),
            value: test_value(*b"valueB"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 3, 0, 0, 0]),
            value: test_value(*b"valueC"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 0, 0, 0, 1, 0]),
            value: test_value(*b"valueD"),
        },
        Trie::Leaf {
            key: test_key([0u8, 2, 0, 0, 0, 0, 0]),
            value: test_value(*b"valueE"),
        },
        Trie::Leaf {
            key: test_key([0u8, 0, 3, 0, 0, 0, 0]),
            value: test_value(*b"valueF"),
        },
    ]
});

type TrieGenerator = fn() -> Result<(Digest, Vec<HashedTrie>), bytesrepr::Error>;

const TEST_TRIE_GENERATORS_LENGTH: usize = 7;

const TEST_TRIE_GENERATORS: [TrieGenerator; TEST_TRIE_GENERATORS_LENGTH] = [
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
    let leaves = hash_test_tries(&*TEST_LEAVES)?;

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

fn put_tries<'a, R, S, E>(environment: &'a R, store: &S, tries: &[HashedTrie]) -> Result<(), E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    if tries.is_empty() {
        return Ok(());
    }
    let mut txn = environment.create_read_write_txn()?;
    for HashedTrie { hash, trie } in tries.iter() {
        store.put(&mut txn, &hash, &trie)?;
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
    fn new(tries: &[HashedTrie]) -> anyhow::Result<Self> {
        let _temp_dir = tempdir()?;
        let environment = LmdbEnvironment::new(
            &_temp_dir.path(),
            DEFAULT_TEST_MAX_DB_SIZE,
            DEFAULT_TEST_MAX_READERS,
            true,
        )?;
        let store = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())?;
        put_tries::<_, _, error::Error>(&environment, &store, tries)?;
        Ok(LmdbTestContext {
            _temp_dir,
            environment,
            store,
        })
    }

    fn update(&self, tries: &[HashedTrie]) -> anyhow::Result<()> {
        put_tries::<_, _, error::Error>(&self.environment, &self.store, tries)?;
        Ok(())
    }
}

fn check_leaves_exist<T, S, E>(
    correlation_id: CorrelationId,
    txn: &T,
    store: &S,
    root: &Digest,
    leaves: &[Trie],
) -> Result<Vec<bool>, E>
where
    T: Readable<Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let mut ret = Vec::new();

    for leaf in leaves {
        if let Trie::Leaf { key, value } = leaf {
            let maybe_value: ReadResult<StoredValue> =
                read::<_, _, E>(correlation_id, txn, store, root, key)?;
            ret.push(ReadResult::Found(value.clone()) == maybe_value)
        } else {
            panic!("leaves should only contain leaves")
        }
    }
    Ok(ret)
}

/// For a given vector of leaves check the merkle proofs exist and are correct
fn check_merkle_proofs<T, S, E>(
    correlation_id: CorrelationId,
    txn: &T,
    store: &S,
    root: &Digest,
    leaves: &[Trie],
) -> Result<Vec<bool>, E>
where
    T: Readable<Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let mut ret = Vec::new();

    for leaf in leaves {
        if let Trie::Leaf { key, value } = leaf {
            let maybe_proof: ReadResult<TrieMerkleProof> =
                read_with_proof::<_, _, E>(correlation_id, txn, store, root, key)?;
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

fn check_keys<T, S, E>(
    correlation_id: CorrelationId,
    txn: &T,
    store: &S,
    root: &Digest,
    leaves: &[Trie],
) -> bool
where
    T: Readable<Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let expected = {
        let mut tmp = leaves
            .iter()
            .filter_map(Trie::key)
            .cloned()
            .collect::<Vec<Key>>();
        tmp.sort();
        tmp
    };
    let actual = {
        let mut tmp = operations::keys::<_, _>(correlation_id, txn, store, root)
            .filter_map(Result::ok)
            .collect::<Vec<Key>>();
        tmp.sort();
        tmp
    };
    expected == actual
}

fn check_leaves<'a, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root: &Digest,
    present: &[Trie],
    absent: &[Trie],
) -> Result<(), E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let txn: R::ReadTransaction = environment.create_read_txn()?;

    assert!(
        check_leaves_exist::<_, _, E>(correlation_id, &txn, store, root, present)?
            .into_iter()
            .all(convert::identity)
    );

    assert!(
        check_merkle_proofs::<_, _, E>(correlation_id, &txn, store, root, present)?
            .into_iter()
            .all(convert::identity)
    );

    assert!(
        check_leaves_exist::<_, _, E>(correlation_id, &txn, store, root, absent)?
            .into_iter()
            .all(bool::not)
    );

    assert!(
        check_merkle_proofs::<_, _, E>(correlation_id, &txn, store, root, absent)?
            .into_iter()
            .all(bool::not)
    );

    assert!(check_keys::<_, _, E>(
        correlation_id,
        &txn,
        store,
        root,
        present,
    ));

    txn.commit()?;
    Ok(())
}

fn write_leaves<'a, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root_hash: &Digest,
    leaves: &[Trie],
) -> Result<Vec<WriteResult>, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore,
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
                write::<_, _, E>(correlation_id, &mut txn, store, &root_hash, &key, &value)?;
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

fn writes_to_n_leaf_empty_trie_had_expected_results<'a, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    states: &[Digest],
    test_leaves: &[Trie],
) -> Result<Vec<Digest>, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let mut states = states.to_vec();

    // Write set of leaves to the trie
    let hashes = write_leaves::<_, _, E>(
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
        check_leaves::<_, _, E>(correlation_id, environment, store, state, used, unused)?;
    }

    Ok(states)
}
