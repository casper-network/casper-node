use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use itertools::Itertools;

use crate::Digest;

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
pub(crate) fn hash_merkle_tree<I>(leaves: I) -> Digest
where
    I: IntoIterator<Item = Digest>,
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
        .unwrap_or((0, Digest::SENTINEL_MERKLE_TREE));
    let leaf_count_bytes = leaf_count.to_le_bytes();
    Digest::hash_pair(leaf_count_bytes, raw_root)
}
