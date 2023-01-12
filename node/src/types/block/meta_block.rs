mod merge_mismatch_error;
mod state;

use std::sync::Arc;

use datasize::DataSize;
use serde::Serialize;

use casper_types::ExecutionResult;

use crate::types::{Block, DeployHash, DeployHeader};

pub(crate) use merge_mismatch_error::MergeMismatchError;
pub(crate) use state::State;

#[derive(Clone, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) struct MetaBlock {
    pub(crate) block: Arc<Block>,
    pub(crate) execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    pub(crate) state: State,
}

impl MetaBlock {
    pub(crate) fn new(
        block: Arc<Block>,
        execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
        state: State,
    ) -> Self {
        MetaBlock {
            block,
            execution_results,
            state,
        }
    }

    pub(crate) fn merge(mut self, other: MetaBlock) -> Result<Self, MergeMismatchError> {
        if self.block != other.block {
            return Err(MergeMismatchError::Block);
        }

        if self.execution_results.is_empty() {
            if !other.execution_results.is_empty() {
                self.execution_results = other.execution_results;
            }
        } else if !other.execution_results.is_empty()
            && self.execution_results != other.execution_results
        {
            return Err(MergeMismatchError::ExecutionResults);
        }

        self.state = self.state.merge(other.state)?;

        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use rand::Rng;

    use casper_types::testing::TestRng;

    use super::*;
    use crate::types::Deploy;

    #[test]
    fn should_merge_when_same_non_empty_execution_results() {
        let mut rng = TestRng::new();

        let block = Arc::new(Block::random(&mut rng));
        let deploy = Deploy::random(&mut rng);
        let execution_results = vec![(*deploy.hash(), deploy.take_header(), rng.gen())];
        let state = State::new_synced();

        let meta_block1 = MetaBlock::new(Arc::clone(&block), execution_results.clone(), state);
        let meta_block2 = MetaBlock::new(Arc::clone(&block), execution_results.clone(), state);

        let merged = meta_block1.clone().merge(meta_block2.clone()).unwrap();

        assert_eq!(merged.block, block);
        assert_eq!(merged.execution_results, execution_results);
        assert_eq!(merged.state, State::new_synced());
        assert_eq!(meta_block2.merge(meta_block1).unwrap(), merged)
    }

    #[test]
    fn should_merge_when_both_empty_execution_results() {
        let mut rng = TestRng::new();

        let block = Arc::new(Block::random(&mut rng));
        let state = State::new();

        let meta_block1 = MetaBlock::new(Arc::clone(&block), vec![], state);
        let meta_block2 = MetaBlock::new(Arc::clone(&block), vec![], state);

        let merged = meta_block1.clone().merge(meta_block2.clone()).unwrap();

        assert_eq!(merged.block, block);
        assert!(merged.execution_results.is_empty());
        assert_eq!(merged.state, state);
        assert_eq!(meta_block2.merge(meta_block1).unwrap(), merged)
    }

    #[test]
    fn should_merge_when_one_empty_execution_results() {
        let mut rng = TestRng::new();

        let block = Arc::new(Block::random(&mut rng));
        let deploy = Deploy::random(&mut rng);
        let execution_results = vec![(*deploy.hash(), deploy.take_header(), rng.gen())];
        let state = State::new_immediate_switch();

        let meta_block1 = MetaBlock::new(Arc::clone(&block), execution_results.clone(), state);
        let meta_block2 = MetaBlock::new(Arc::clone(&block), vec![], state);

        let merged = meta_block1.clone().merge(meta_block2.clone()).unwrap();

        assert_eq!(merged.block, block);
        assert_eq!(merged.execution_results, execution_results);
        assert_eq!(merged.state, state);
        assert_eq!(meta_block2.merge(meta_block1).unwrap(), merged)
    }

    #[test]
    fn should_fail_to_merge_different_blocks() {
        let mut rng = TestRng::new();

        let block1 = Arc::new(Block::random(&mut rng));
        let block2 = Arc::new(Block::random_with_specifics(
            &mut rng,
            block1.header().era_id().successor(),
            block1.height() + 1,
            block1.protocol_version(),
            true,
            iter::empty(),
        ));
        let deploy = Deploy::random(&mut rng);
        let execution_results = vec![(*deploy.hash(), deploy.take_header(), rng.gen())];
        let state = State::new();

        let meta_block1 = MetaBlock::new(block1, execution_results.clone(), state);
        let meta_block2 = MetaBlock::new(block2, execution_results, state);

        assert!(matches!(
            meta_block1.clone().merge(meta_block2.clone()),
            Err(MergeMismatchError::Block)
        ));
        assert!(matches!(
            meta_block2.merge(meta_block1),
            Err(MergeMismatchError::Block)
        ));
    }

    #[test]
    fn should_fail_to_merge_different_execution_results() {
        let mut rng = TestRng::new();

        let block = Arc::new(Block::random(&mut rng));
        let deploy1 = Deploy::random(&mut rng);
        let execution_results1 = vec![(*deploy1.hash(), deploy1.take_header(), rng.gen())];
        let deploy2 = Deploy::random(&mut rng);
        let execution_results2 = vec![(*deploy2.hash(), deploy2.take_header(), rng.gen())];
        let state = State::new();

        let meta_block1 = MetaBlock::new(Arc::clone(&block), execution_results1, state);
        let meta_block2 = MetaBlock::new(Arc::clone(&block), execution_results2, state);

        assert!(matches!(
            meta_block1.clone().merge(meta_block2.clone()),
            Err(MergeMismatchError::ExecutionResults)
        ));
        assert!(matches!(
            meta_block2.merge(meta_block1),
            Err(MergeMismatchError::ExecutionResults)
        ));
    }
}
