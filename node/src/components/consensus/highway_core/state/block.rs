use datasize::DataSize;
use serde::Serialize;

use super::State;
use crate::components::consensus::traits::Context;

/// A block: Chains of blocks are the consensus values in the CBC Casper sense.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Block<C>
where
    C: Context,
{
    /// The total number of ancestors, i.e. the height in the blockchain.
    pub(crate) height: u64,
    /// The payload, e.g. a list of transactions.
    pub(crate) value: C::ConsensusValue,
    /// A skip list index of the block's ancestors.
    ///
    /// For every `p = 1 << i` that divides `height`, this contains an `i`-th entry pointing to the
    /// ancestor with `height - p`.
    pub(crate) skip_idx: Vec<C::Hash>,
}

impl<C: Context> Block<C> {
    /// Creates a new block with the given parent and values. Panics if parent does not exist.
    pub(crate) fn new(
        parent_hash: Option<C::Hash>,
        value: C::ConsensusValue,
        state: &State<C>,
    ) -> Block<C> {
        let (parent, mut skip_idx) = match parent_hash {
            None => return Block::initial(value),
            Some(hash) => (state.block(&hash), vec![hash]),
        };
        // In a trillion years, we need to make block height u128.
        #[allow(clippy::integer_arithmetic)]
        let height = parent.height + 1;
        for i in 0..height.trailing_zeros() as usize {
            let ancestor = state.block(&skip_idx[i]);
            skip_idx.push(ancestor.skip_idx[i]);
        }
        Block {
            height,
            value,
            skip_idx,
        }
    }

    /// Returns the block's parent, or `None` if it has height 0.
    pub(crate) fn parent(&self) -> Option<&C::Hash> {
        self.skip_idx.first()
    }

    fn initial(value: C::ConsensusValue) -> Block<C> {
        Block {
            height: 0,
            value,
            skip_idx: vec![],
        }
    }
}
