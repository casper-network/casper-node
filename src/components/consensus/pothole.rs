mod chain;
#[cfg(test)]
mod tests;

pub(crate) use chain::BlockIndex;
use chain::Chain;
use std::{
    collections::BTreeSet,
    fmt::Debug,
    time::{Duration, Instant},
};

/// A trait for block types to implement
pub(crate) trait Block: Clone + Debug {}
impl<T> Block for T where T: Clone + Debug {}

pub(crate) trait NodeId: Clone + Debug + PartialEq + Ord {}
impl<T> NodeId for T where T: Clone + Debug + PartialEq + Ord {}

/// An identifier for a timer set to fire at a later moment
pub(crate) type TimerId = u64;

/// Possible effects that could result from the consensus operations
#[derive(Debug)]
pub(crate) enum PotholeResult<B> {
    /// A request for a timer to be scheduled
    ScheduleTimer(TimerId, Instant),
    /// A request for a block to be proposed
    CreateNewBlock,
    /// A notification that a block has been finalized
    FinalizedBlock(BlockIndex, B),
}

/// The state of the consensus protocol
#[derive(Debug)]
pub(crate) enum Pothole<B: Block> {
    Dictator {
        chain: Chain<B>,
        // The ID of a timer that fires when we are supposed to propose a new block
        block_timer: TimerId,
    },
    Follower {
        chain: Chain<B>,
    },
}

const BLOCK_PROPOSE_DURATION: Duration = Duration::from_secs(10);

impl<B: Block> Pothole<B> {
    /// Creates a new instance of the protocol. If this node is the first (lexicographically) among
    /// the peers, it becomes the dictator (the node determining the order of blocks). Returns the
    /// protocol instance along with some possible side-effects.
    pub(crate) fn new<N: NodeId>(
        our_id: &N,
        all_nodes: &BTreeSet<N>,
    ) -> (Self, Vec<PotholeResult<B>>) {
        let dictator = Some(our_id) == all_nodes.iter().next();
        let pothole = if dictator {
            Pothole::Dictator {
                chain: Chain::new(),
                block_timer: 0,
            }
        } else {
            Pothole::Follower {
                chain: Chain::new(),
            }
        };
        let results = if dictator {
            vec![PotholeResult::ScheduleTimer(
                0, // TODO: do timer ids come from outside or do we set them arbitrarily?
                Instant::now() + BLOCK_PROPOSE_DURATION,
            )]
        } else {
            vec![]
        };
        (pothole, results)
    }

    /// Handles a timer event (scheduled according to an earlier ScheduleTimer request).
    pub(crate) fn handle_timer(&mut self, timer: TimerId) -> Vec<PotholeResult<B>> {
        match self {
            Pothole::Dictator { block_timer, .. } if *block_timer == timer => vec![
                PotholeResult::CreateNewBlock,
                PotholeResult::ScheduleTimer(timer, Instant::now() + BLOCK_PROPOSE_DURATION),
            ],
            _ => Vec::new(),
        }
    }

    /// Proposes a new block for the chain.
    pub(crate) fn propose_block(&mut self, block: B) -> Vec<PotholeResult<B>> {
        match self {
            Pothole::Dictator { chain, .. } => {
                let index = chain.append(block.clone());
                vec![PotholeResult::FinalizedBlock(index, block)]
            }
            Pothole::Follower { .. } => Vec::new(),
        }
    }

    /// Handles a notification about a new block having been finalized.
    pub(crate) fn handle_new_block(
        &mut self,
        index: BlockIndex,
        block: B,
    ) -> Result<Vec<PotholeResult<B>>, BTreeSet<BlockIndex>> {
        match self {
            Pothole::Dictator { .. } => Ok(Vec::new()),
            Pothole::Follower { chain } => match chain.insert(index, block.clone()) {
                Ok(None) => Ok(vec![PotholeResult::FinalizedBlock(index, block)]),
                Ok(Some(_)) => Ok(vec![]),
                Err(next_index) => Err((next_index..index).collect()),
            },
        }
    }

    /// Returns a reference to the Chain container
    pub(crate) fn chain(&self) -> &Chain<B> {
        match self {
            Pothole::Dictator { chain, .. } => chain,
            Pothole::Follower { chain } => chain,
        }
    }
}
