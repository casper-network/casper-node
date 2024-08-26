use std::borrow::Cow;

use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    Digest, Pointer,
};

use crate::global_state::{
    transaction_source::{Readable, Writable},
    trie::{PointerBlock, Trie, RADIX},
};

use super::{operations::common_prefix, TrieStore};

#[derive(Clone, Debug, thiserror::Error, Eq, PartialEq)]
pub enum CacheError {
    /// Root not found.
    #[error("Root not found: {0:?}")]
    RootNotFound(Digest),
}

// Pointer used by the cache to determine if the node is stored or is loaded in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CachePointer<K, V> {
    InMem(TrieCacheNode<K, V>),
    Stored(Pointer),
}

impl<K, V> CachePointer<K, V> {
    /// Loads the node in memory from the specified store if it's not already loaded.
    /// Returns an error if the node can't be found in the store.
    fn load_from_store<T, S, E>(&mut self, txn: &T, store: &S) -> Result<(), E>
    where
        K: FromBytes,
        V: FromBytes,
        T: Readable<Handle = S::Handle>,
        S: TrieStore<K, V>,
        S::Error: From<T::Error>,
        E: From<S::Error> + From<bytesrepr::Error> + From<CacheError>,
    {
        if let CachePointer::Stored(pointer) = self {
            let Some(stored_node) = store.get(txn, pointer.hash())? else {
                return Err(CacheError::RootNotFound(pointer.into_hash()).into());
            };
            let trie_cache_node = stored_node.into();
            *self = CachePointer::InMem(trie_cache_node);
        }
        Ok(())
    }
}

/// A node representation used by the cache. This follows the Trie implementation for easy
/// conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TrieCacheNode<K, V> {
    Leaf {
        key: K,
        value: V,
    },
    Branch {
        pointer_block: Vec<Option<CachePointer<K, V>>>,
    },
    Extension {
        affix: Bytes,
        pointer: Box<CachePointer<K, V>>,
    },
}

impl<K, V> From<Trie<K, V>> for TrieCacheNode<K, V> {
    fn from(node: Trie<K, V>) -> Self {
        match node {
            Trie::Leaf { key, value } => Self::Leaf { key, value },
            Trie::Node { pointer_block } => {
                let mut new_pointer_block = Vec::with_capacity(RADIX);
                for i in 0..RADIX {
                    new_pointer_block.push(pointer_block[i].map(|ptr| CachePointer::Stored(ptr)));
                }
                Self::Branch {
                    pointer_block: new_pointer_block,
                }
            }
            Trie::Extension { affix, pointer } => Self::Extension {
                affix,
                pointer: Box::new(CachePointer::Stored(pointer)),
            },
        }
    }
}

// An in-memory cache for Trie nodes that is backed up by a store.
pub struct TrieCache<'a, K, V, S> {
    root: TrieCacheNode<K, V>,
    store: &'a S,
}

impl<'a, K, V, S> TrieCache<'a, K, V, S>
where
    K: ToBytes + FromBytes + Clone + Eq,
    V: ToBytes + FromBytes + Clone + Eq,
    S: TrieStore<K, V> + 'a,
{
    pub fn new<T, E>(txn: &T, store: &'a S, root: &Digest) -> Result<Self, E>
    where
        T: Readable<Handle = S::Handle>,
        S::Error: From<T::Error>,
        E: From<S::Error> + From<bytesrepr::Error> + From<CacheError>,
    {
        match store.get(txn, root)? {
            Some(node) => Ok(Self {
                root: node.into(),
                store,
            }),
            None => Err(CacheError::RootNotFound(*root).into()),
        }
    }

    pub fn insert<T, E>(&mut self, key: K, value: V, txn: &T) -> Result<(), E>
    where
        T: Readable<Handle = S::Handle>,
        S::Error: From<T::Error>,
        E: From<S::Error> + From<bytesrepr::Error> + From<CacheError>,
    {
        let path: Vec<u8> = key.to_bytes()?;

        let mut depth: usize = 0;
        let mut current = &mut self.root;

        while depth < path.len() {
            match current {
                TrieCacheNode::Branch { pointer_block } => {
                    let index: usize = {
                        assert!(depth < path.len(), "depth must be < {}", path.len());
                        path[depth].into()
                    };

                    let pointer = &mut pointer_block[index];
                    if let Some(next) = pointer {
                        if depth == path.len() - 1 {
                            let leaf = TrieCacheNode::Leaf { key, value };
                            *next = CachePointer::InMem(leaf);
                            return Ok(());
                        } else {
                            depth += 1;

                            next.load_from_store::<_, _, E>(txn, self.store)?;
                            if let CachePointer::InMem(next) = next {
                                current = next;
                            } else {
                                unreachable!("Stored pointer should have been converted");
                            }
                        }
                    } else {
                        let leaf = TrieCacheNode::Leaf { key, value };
                        let _ = std::mem::replace(pointer, Some(CachePointer::InMem(leaf)));
                        return Ok(());
                    }
                }
                TrieCacheNode::Leaf {
                    key: old_key,
                    value: old_value,
                } => {
                    if *old_key == key {
                        *old_value = value;
                    } else {
                        let mut pointer_block = Vec::with_capacity(RADIX);
                        pointer_block.resize_with(RADIX, || None::<CachePointer<K, V>>);
                        let old_key_bytes = old_key.to_bytes()?;

                        let shared_path = common_prefix(&old_key_bytes, &path);

                        let existing_idx = old_key_bytes[shared_path.len()] as usize;
                        pointer_block[existing_idx] =
                            Some(CachePointer::InMem(TrieCacheNode::Leaf {
                                key: old_key.clone(),
                                value: old_value.clone(),
                            }));

                        let new_idx = path[shared_path.len()] as usize;
                        pointer_block[new_idx] =
                            Some(CachePointer::InMem(TrieCacheNode::Leaf { key, value }));

                        let new_affix = { &shared_path[depth..] };
                        *current = if !new_affix.is_empty() {
                            TrieCacheNode::Extension {
                                affix: Bytes::from(new_affix),
                                pointer: Box::new(CachePointer::InMem(TrieCacheNode::Branch {
                                    pointer_block,
                                })),
                            }
                        } else {
                            TrieCacheNode::Branch { pointer_block }
                        };
                    }
                    return Ok(());
                }
                TrieCacheNode::Extension { affix, ref pointer }
                    if path.len() < depth + affix.len()
                        || affix.as_ref() != &path[depth..depth + affix.len()] =>
                {
                    // We might be trying to store a key that is shorter than the keys that are
                    // already stored. In this case, we would need to split this extension.
                    // We also need to split this extension if the affix changes.

                    // Is there something common between the new key and the old key?
                    let shared_prefix = common_prefix(affix, &path[depth..]);

                    // Need to split the node at the byte that is different.
                    let mut pointer_block = Vec::with_capacity(RADIX);
                    pointer_block.resize_with(RADIX, || None::<CachePointer<K, V>>);

                    // Add the new key under a leaf where the paths diverge.
                    pointer_block[path[depth + shared_prefix.len()] as usize] =
                        Some(CachePointer::InMem(TrieCacheNode::Leaf { key, value }));

                    let post_branch_affix = &affix[shared_prefix.len() + 1..];
                    if !post_branch_affix.is_empty() {
                        let post_extension = TrieCacheNode::Extension {
                            affix: Bytes::from(post_branch_affix),
                            pointer: pointer.clone(),
                        };
                        let existing_idx = affix[shared_prefix.len()] as usize;
                        pointer_block[existing_idx] = Some(CachePointer::InMem(post_extension));
                    } else {
                        let existing_idx = affix[shared_prefix.len()] as usize;
                        pointer_block[existing_idx] = Some(*pointer.clone());
                    }

                    let new_branch = TrieCacheNode::Branch { pointer_block };
                    let next = if !shared_prefix.is_empty() {
                        // Create an extension node with the common part
                        TrieCacheNode::Extension {
                            affix: Bytes::from(shared_prefix),
                            pointer: Box::new(CachePointer::InMem(new_branch)),
                        }
                    } else {
                        new_branch
                    };

                    *current = next;
                    return Ok(());
                }
                TrieCacheNode::Extension {
                    affix,
                    ref mut pointer,
                } => {
                    depth += affix.len();
                    pointer.load_from_store::<_, _, E>(txn, self.store)?;
                    if let CachePointer::InMem(next) = pointer.as_mut() {
                        current = next;
                    } else {
                        unreachable!("Stored pointer should have been converted");
                    }
                }
            }
        }
        Ok(())
    }

    fn traverse_and_store<T, E>(
        node: TrieCacheNode<K, V>,
        txn: &mut T,
        store: &S,
    ) -> Result<Pointer, E>
    where
        T: Readable<Handle = S::Handle> + Writable<Handle = S::Handle>,
        S::Error: From<T::Error>,
        E: From<S::Error> + From<bytesrepr::Error> + From<CacheError>,
    {
        match node {
            TrieCacheNode::Leaf { key, value } => {
                let trie_leaf = Trie::leaf(key, value);
                let (hash, trie_bytes) = trie_leaf.trie_hash_and_bytes()?;
                store.put_raw(txn, &hash, Cow::from(trie_bytes))?;
                Ok(Pointer::LeafPointer(hash))
            }
            TrieCacheNode::Branch { mut pointer_block } => {
                let mut trie_pointer_block = PointerBlock::new();
                for i in 0..RADIX {
                    trie_pointer_block[i] = Option::take(&mut pointer_block[i])
                        .map(|child| match child {
                            CachePointer::InMem(in_mem_child) => {
                                Self::traverse_and_store::<_, E>(in_mem_child, txn, store)
                            }
                            CachePointer::Stored(ptr) => Ok(ptr),
                        })
                        .transpose()?;
                }

                let trie_node = Trie::<K, V>::Node {
                    pointer_block: Box::new(trie_pointer_block),
                };
                let (hash, trie_bytes) = trie_node.trie_hash_and_bytes()?;
                store.put_raw(txn, &hash, Cow::from(trie_bytes))?;
                Ok(Pointer::NodePointer(hash))
            }
            TrieCacheNode::Extension { pointer, affix } => {
                let pointer = match *pointer {
                    CachePointer::InMem(in_mem_ptr) => {
                        Self::traverse_and_store::<_, E>(in_mem_ptr, txn, store)
                    }
                    CachePointer::Stored(ptr) => Ok(ptr),
                }?;

                let trie_extension = Trie::<K, V>::extension(affix.to_vec(), pointer);
                let (hash, trie_bytes) = trie_extension.trie_hash_and_bytes()?;
                store.put_raw(txn, &hash, Cow::from(trie_bytes))?;
                Ok(Pointer::NodePointer(hash))
            }
        }
    }

    pub fn store_cache<T, E>(self, txn: &mut T) -> Result<Digest, E>
    where
        T: Readable<Handle = S::Handle> + Writable<Handle = S::Handle>,
        S::Error: From<T::Error>,
        E: From<S::Error> + From<bytesrepr::Error> + From<CacheError>,
    {
        Self::traverse_and_store::<_, E>(self.root, txn, self.store)
            .map(|root_pointer| root_pointer.into_hash())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<'a, K, V, S> TrieCache<'a, K, V, S>
    where
        K: ToBytes + FromBytes + Clone + Eq,
        V: ToBytes + FromBytes + Clone + Eq,
        S: TrieStore<K, V>,
    {
        fn traverse(node: TrieCacheNode<K, V>) -> Pointer {
            match node {
                TrieCacheNode::Leaf { key, value } => {
                    // Process the leaf node
                    let trie_leaf = Trie::leaf(key, value);
                    let hash = trie_leaf.trie_hash().unwrap();
                    Pointer::LeafPointer(hash)
                }
                TrieCacheNode::Branch { mut pointer_block } => {
                    let mut trie_pointer_block = PointerBlock::new();
                    for i in 0..RADIX {
                        trie_pointer_block[i] = Option::take(pointer_block.get_mut(i).unwrap())
                            .map(|child| match child {
                                CachePointer::InMem(in_mem_child) => Self::traverse(in_mem_child),
                                CachePointer::Stored(ptr) => ptr,
                            });
                    }

                    let trie_node = Trie::<K, V>::Node {
                        pointer_block: Box::new(trie_pointer_block),
                    };
                    let hash = trie_node.trie_hash().unwrap();
                    Pointer::NodePointer(hash)
                }
                TrieCacheNode::Extension { pointer, affix } => {
                    let pointer = match *pointer {
                        CachePointer::InMem(in_mem_ptr) => Self::traverse(in_mem_ptr),
                        CachePointer::Stored(ptr) => ptr,
                    };

                    let trie_extension = Trie::<K, V>::extension(affix.to_vec(), pointer);
                    let hash = trie_extension.trie_hash().unwrap();
                    Pointer::NodePointer(hash)
                }
            }
        }

        pub fn calculate_root_hash(self) -> Digest {
            Self::traverse(self.root).into_hash()
        }
    }
}
