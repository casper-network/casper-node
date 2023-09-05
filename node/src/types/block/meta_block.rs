mod merge_mismatch_error;
mod state;

use std::sync::Arc;

use datasize::DataSize;
use serde::Serialize;

use casper_types::{
    execution::ExecutionResult, ActivationPoint, BlockV2, DeployHash, DeployHeader,
};

pub(crate) use merge_mismatch_error::MergeMismatchError;
pub(crate) use state::State;

/// A block along with its execution results and state recording which actions have been taken
/// related to the block.
///
/// Some or all of these actions should be taken after a block is formed on a node via:
/// * execution (ContractRuntime executing a FinalizedBlock)
/// * accumulation (BlockAccumulator receiving a gossiped block and its finality signatures)
/// * historical sync (BlockSynchronizer fetching all data relating to a block)
#[derive(Clone, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) struct MetaBlock {
    pub(crate) block: Arc<BlockV2>,
    pub(crate) execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    pub(crate) state: State,
}

impl MetaBlock {
    pub(crate) fn new(
        block: Arc<BlockV2>,
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

    /// Is this a switch block?
    pub(crate) fn is_switch_block(&self) -> bool {
        self.block.is_switch_block()
    }

    /// Is this the last block before a protocol version upgrade?
    pub(crate) fn is_upgrade_boundary(&self, activation_point: ActivationPoint) -> bool {
        match activation_point {
            ActivationPoint::EraId(era_id) => {
                self.is_switch_block() && self.block.era_id().successor() == era_id
            }
            ActivationPoint::Genesis(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use casper_types::{execution::ExecutionResultV2, testing::TestRng, Deploy};

    use super::*;
    use crate::types::TestBlockBuilder;

    #[test]
    fn should_merge_when_same_non_empty_execution_results() {
        let rng = &mut TestRng::new();

        let block = Arc::new(TestBlockBuilder::new().build(rng));
        let deploy = Deploy::random(rng);
        let execution_results = vec![(
            *deploy.hash(),
            deploy.take_header(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
        )];
        let state = State::new_already_stored();

        let meta_block1 = MetaBlock::new(Arc::clone(&block), execution_results.clone(), state);
        let meta_block2 = MetaBlock::new(Arc::clone(&block), execution_results.clone(), state);

        let merged = meta_block1.clone().merge(meta_block2.clone()).unwrap();

        assert_eq!(merged.block, block);
        assert_eq!(merged.execution_results, execution_results);
        assert_eq!(merged.state, State::new_already_stored());
        assert_eq!(meta_block2.merge(meta_block1).unwrap(), merged)
    }

    #[test]
    fn should_merge_when_both_empty_execution_results() {
        let rng = &mut TestRng::new();

        let block = Arc::new(TestBlockBuilder::new().build(rng));
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
        let rng = &mut TestRng::new();

        let block = Arc::new(TestBlockBuilder::new().build(rng));
        let deploy = Deploy::random(rng);
        let execution_results = vec![(
            *deploy.hash(),
            deploy.take_header(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
        )];
        let state = State::new_not_to_be_gossiped();

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
        let rng = &mut TestRng::new();

        let block1 = Arc::new(TestBlockBuilder::new().build(rng));
        let block2 = Arc::new(
            TestBlockBuilder::new()
                .era(block1.era_id().successor())
                .height(block1.height() + 1)
                .switch_block(true)
                .build(rng),
        );
        let deploy = Deploy::random(rng);
        let execution_results = vec![(
            *deploy.hash(),
            deploy.take_header(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
        )];
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
        let rng = &mut TestRng::new();

        let block = Arc::new(TestBlockBuilder::new().build(rng));
        let deploy1 = Deploy::random(rng);
        let execution_results1 = vec![(
            *deploy1.hash(),
            deploy1.take_header(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
        )];
        let deploy2 = Deploy::random(rng);
        let execution_results2 = vec![(
            *deploy2.hash(),
            deploy2.take_header(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
        )];
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
