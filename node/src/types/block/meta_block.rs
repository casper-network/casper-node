mod merge_mismatch_error;
mod state;

use std::{convert::TryFrom, sync::Arc};

use crate::types::TransactionHeader;
use datasize::DataSize;
use serde::Serialize;

use casper_types::{
    execution::ExecutionResult, ActivationPoint, Block, BlockHash, BlockV2, EraId, TransactionHash,
};

pub(crate) use merge_mismatch_error::MergeMismatchError;
pub(crate) use state::State;

use crate::contract_runtime::ExecutionArtifact;

/// A block along with its execution results and state recording which actions have been taken
/// related to the block.
///
/// Some or all of these actions should be taken after a block is formed on a node via:
/// * execution (ContractRuntime executing a FinalizedBlock)
/// * accumulation (BlockAccumulator receiving a gossiped block and its finality signatures)
/// * historical sync (BlockSynchronizer fetching all data relating to a block)
#[derive(Clone, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) enum MetaBlock {
    Forward(ForwardMetaBlock),
    Historical(HistoricalMetaBlock),
}

impl MetaBlock {
    pub(crate) fn new_forward(
        block: Arc<BlockV2>,
        execution_results: Vec<ExecutionArtifact>,
        state: State,
    ) -> Self {
        Self::Forward(ForwardMetaBlock {
            block,
            execution_results,
            state,
        })
    }

    pub(crate) fn new_historical(
        block: Arc<Block>,
        execution_results: Vec<(TransactionHash, TransactionHeader, ExecutionResult)>,
        state: State,
    ) -> Self {
        Self::Historical(HistoricalMetaBlock {
            block,
            execution_results,
            state,
        })
    }

    pub(crate) fn height(&self) -> u64 {
        match &self {
            MetaBlock::Forward(meta_block) => meta_block.block.height(),
            MetaBlock::Historical(meta_block) => meta_block.block.height(),
        }
    }

    pub(crate) fn era_id(&self) -> EraId {
        match &self {
            MetaBlock::Forward(meta_block) => meta_block.block.era_id(),
            MetaBlock::Historical(meta_block) => meta_block.block.era_id(),
        }
    }

    pub(crate) fn is_switch_block(&self) -> bool {
        match &self {
            MetaBlock::Forward(meta_block) => meta_block.block.is_switch_block(),
            MetaBlock::Historical(meta_block) => meta_block.block.is_switch_block(),
        }
    }

    pub(crate) fn hash(&self) -> BlockHash {
        match &self {
            MetaBlock::Forward(meta_block) => *meta_block.block.hash(),
            MetaBlock::Historical(meta_block) => *meta_block.block.hash(),
        }
    }

    pub(crate) fn mut_state(&mut self) -> &mut State {
        match self {
            MetaBlock::Forward(meta_block) => &mut meta_block.state,
            MetaBlock::Historical(meta_block) => &mut meta_block.state,
        }
    }

    pub(crate) fn state(&self) -> &State {
        match &self {
            MetaBlock::Forward(meta_block) => &meta_block.state,
            MetaBlock::Historical(meta_block) => &meta_block.state,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) struct ForwardMetaBlock {
    pub(crate) block: Arc<BlockV2>,
    pub(crate) execution_results: Vec<ExecutionArtifact>,
    pub(crate) state: State,
}

#[derive(Clone, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) struct HistoricalMetaBlock {
    pub(crate) block: Arc<Block>,
    pub(crate) execution_results: Vec<(TransactionHash, TransactionHeader, ExecutionResult)>,
    pub(crate) state: State,
}

impl ForwardMetaBlock {
    pub(crate) fn merge(mut self, other: ForwardMetaBlock) -> Result<Self, MergeMismatchError> {
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

impl TryFrom<MetaBlock> for ForwardMetaBlock {
    type Error = String;

    fn try_from(value: MetaBlock) -> Result<Self, Self::Error> {
        match value {
            MetaBlock::Forward(meta_block) => Ok(meta_block),
            MetaBlock::Historical(_) => {
                Err("Could not convert Historical Meta Block to Forward Meta Block".to_string())
            }
        }
    }
}

impl From<ForwardMetaBlock> for MetaBlock {
    fn from(value: ForwardMetaBlock) -> Self {
        Self::Forward(value)
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use casper_types::{
        execution::ExecutionResultV2, testing::TestRng, TestBlockBuilder, TransactionV1,
    };

    use super::*;

    #[test]
    fn should_merge_when_same_non_empty_execution_results() {
        let rng = &mut TestRng::new();

        let block = Arc::new(TestBlockBuilder::new().build(rng));
        let txn = TransactionV1::random(rng);
        let execution_results = vec![ExecutionArtifact::new(
            TransactionHash::V1(*txn.hash()),
            (&txn).into(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
            Vec::new(),
        )];
        let state = State::new_already_stored();

        let meta_block1: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), execution_results.clone(), state)
                .try_into()
                .unwrap();
        let meta_block2: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), execution_results.clone(), state)
                .try_into()
                .unwrap();

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

        let meta_block1: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), vec![], state)
                .try_into()
                .unwrap();
        let meta_block2: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), vec![], state)
                .try_into()
                .unwrap();

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
        let txn = TransactionV1::random(rng);
        let execution_results = vec![ExecutionArtifact::new(
            TransactionHash::V1(*txn.hash()),
            (&txn).into(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
            Vec::new(),
        )];
        let state = State::new_not_to_be_gossiped();

        let meta_block1: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), execution_results.clone(), state)
                .try_into()
                .unwrap();
        let meta_block2: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), vec![], state)
                .try_into()
                .unwrap();

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
        let txn = TransactionV1::random(rng);
        let execution_results = vec![ExecutionArtifact::new(
            TransactionHash::V1(*txn.hash()),
            (&txn).into(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
            Vec::new(),
        )];
        let state = State::new();

        let meta_block1: ForwardMetaBlock =
            MetaBlock::new_forward(block1, execution_results.clone(), state)
                .try_into()
                .unwrap();
        let meta_block2: ForwardMetaBlock =
            MetaBlock::new_forward(block2, execution_results, state)
                .try_into()
                .unwrap();

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
        let txn1 = TransactionV1::random(rng);
        let execution_results1 = vec![ExecutionArtifact::new(
            TransactionHash::V1(*txn1.hash()),
            (&txn1).into(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
            Vec::new(),
        )];
        let txn2 = TransactionV1::random(rng);
        let execution_results2 = vec![ExecutionArtifact::new(
            TransactionHash::V1(*txn2.hash()),
            (&txn2).into(),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
            Vec::new(),
        )];
        let state = State::new();

        let meta_block1: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), execution_results1, state)
                .try_into()
                .unwrap();
        let meta_block2: ForwardMetaBlock =
            MetaBlock::new_forward(Arc::clone(&block), execution_results2, state)
                .try_into()
                .unwrap();

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
