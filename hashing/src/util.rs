use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use itertools::Itertools;

use crate::{blake2b_hash::Blake2bHash, Digest};

/// Sentinel hash to be used for hashing options in the case of [None].
pub(crate) const SENTINEL0: Blake2bHash = Blake2bHash([0u8; 32]);
/// Sentinel hash to be used by [hash_slice_rfold]. Terminates the fold.
pub(crate) const SENTINEL1: Blake2bHash = Blake2bHash([1u8; 32]);
/// Sentinel hash to be used by [hash_vec_merkle_tree] in the case of an empty list.
pub(crate) const SENTINEL2: Blake2bHash = Blake2bHash([2u8; 32]);

/// Hashes a `impl IntoIterator` of [`Blake2bHash`]s into a single [`Blake2bHash`] by constructing a
/// [Merkle tree][1]. Reduces pairs of elements in the [`Vec`] by repeatedly calling [hash_pair].
///
/// The pattern of hashing is as follows.  It is akin to [graph reduction][2]:
///
/// ```text
/// a b c d e f
/// |/  |/  |/
/// g   h   i
/// | /   /
/// |/   /
/// j   k
/// | /
/// |/
/// l
/// ```
///
/// Finally hashes the number of elements resulting hash. In the example above the final output
/// would be `hash_pair(u64_as_slice(6), l)`.
///
/// Returns [`SENTINEL2`] when the input is empty.
///
/// [1]: https://en.wikipedia.org/wiki/Merkle_tree
/// [2]: https://en.wikipedia.org/wiki/Graph_reduction
pub(crate) fn hash_merkle_tree<I>(leaves: I) -> Blake2bHash
where
    I: IntoIterator<Item = Blake2bHash>,
{
    let (leaf_count, raw_root) = leaves
        .into_iter()
        .map(|x| (1u64, x))
        .tree_fold1(|(mut count_x, mut hash_x), (count_y, hash_y)| {
            let mut hasher = VarBlake2b::new(Digest::LENGTH).unwrap();
            hasher.update(&hash_x);
            hasher.update(&hash_y);
            hasher.finalize_variable(|slice| {
                hash_x.0.copy_from_slice(slice);
            });
            (count_x + count_y, hash_x)
        })
        .unwrap_or((0, SENTINEL2));
    let leaf_count_bytes = leaf_count.to_le_bytes();
    hash_pair(leaf_count_bytes, raw_root)
}

/// Creates a 32-byte hash digest from a given a piece of data
pub(crate) fn blake2b_hash<T: AsRef<[u8]>>(data: T) -> Blake2bHash {
    let mut result = [0; Digest::LENGTH];

    let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");
    hasher.update(data);
    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    Blake2bHash(result)
}

/// Hashes a pair of byte slices into a single [`Blake2bHash`]
pub(crate) fn hash_pair<T: AsRef<[u8]>, U: AsRef<[u8]>>(data1: T, data2: U) -> Blake2bHash {
    let mut result = [0; Digest::LENGTH];
    let mut hasher = VarBlake2b::new(Digest::LENGTH).unwrap();
    hasher.update(data1);
    hasher.update(data2);
    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    Blake2bHash(result)
}
