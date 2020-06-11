use std::{
    collections::{BTreeSet, VecDeque},
    convert::{From, TryFrom},
    hash::Hash,
    marker::PhantomData,
    mem,
};

use derive_more::{Deref, DerefMut};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::super::consensus_protocol::{
    ConsensusContext, ConsensusProtocol, ConsensusProtocolResult, NodeId as ConsensusNodeId,
    TimerId,
};
use super::super::consensus_service::traits::{EraId, MessageWireFormat};
use super::super::pothole::{Block, BlockIndex, Pothole, PotholeResult};
use super::super::synchronizer::{
    DependencySpec, HandleNewItemResult, ItemWithId, NodeId, ProtocolState, Synchronizer,
    SynchronizerMessage,
};

#[derive(Debug)]
pub(crate) enum PotholeMessage<B> {
    NewBlock(BlockIndex, B),
}

#[derive(Debug, Deref, DerefMut)]
pub(crate) struct PotholeWrapper<B: Block> {
    finalized_block_queue: VecDeque<(BlockIndex, B)>,
    #[deref]
    #[deref_mut]
    pothole: Pothole<B>,
}

impl<B: Block> PotholeWrapper<B> {
    pub(crate) fn new(pothole: Pothole<B>) -> Self {
        Self {
            pothole,
            finalized_block_queue: Default::default(),
        }
    }

    pub(crate) fn poll(&mut self) -> Option<(BlockIndex, B)> {
        self.finalized_block_queue.pop_front()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PotholeDepSpec<B> {
    to_request: BTreeSet<BlockIndex>,
    requested: BTreeSet<BlockIndex>,
    _block: PhantomData<B>,
}

impl<B> PotholeDepSpec<B> {
    pub(crate) fn new(deps: BTreeSet<BlockIndex>) -> Self {
        Self {
            to_request: deps,
            requested: Default::default(),
            _block: PhantomData,
        }
    }
}

impl<B: Block + Hash + Eq> DependencySpec for PotholeDepSpec<B> {
    type DependencyDescription = BlockIndex;
    type ItemId = BlockIndex;
    type Item = B;

    fn next_dependency(&mut self) -> Option<BlockIndex> {
        let mut deps = mem::take(&mut self.to_request).into_iter();
        let next_dep = deps.next();
        self.to_request = deps.collect();
        if let Some(dep) = next_dep {
            self.requested.insert(dep);
        }
        next_dep
    }

    fn resolve_dependency(&mut self, dep: BlockIndex) -> bool {
        self.to_request.remove(&dep) || self.requested.remove(&dep)
    }

    fn all_resolved(&self) -> bool {
        self.to_request.is_empty() && self.requested.is_empty()
    }
}

impl<B: Block + Hash + Eq> ProtocolState for PotholeWrapper<B> {
    type DepSpec = PotholeDepSpec<B>;

    fn get_dependency(&self, dep: &BlockIndex) -> Option<ItemWithId<PotholeDepSpec<B>>> {
        self.pothole
            .chain()
            .get_block(*dep)
            .map(|block| ItemWithId {
                item_id: *dep,
                item: block.clone(),
            })
    }

    fn handle_new_item(
        &mut self,
        item_id: BlockIndex,
        item: B,
    ) -> HandleNewItemResult<PotholeDepSpec<B>> {
        match self.pothole.handle_new_block(item_id, item) {
            Ok(messages) => {
                for message in messages {
                    if let PotholeResult::FinalizedBlock(index, block) = message {
                        self.finalized_block_queue.push_back((index, block));
                    }
                }
                HandleNewItemResult::Accepted
            }
            Err(deps) => HandleNewItemResult::DependenciesMissing(PotholeDepSpec::new(deps)),
        }
    }
}

pub(crate) struct PotholeContext<N, B> {
    _n: PhantomData<N>,
    _b: PhantomData<B>,
}

impl<N: NodeId, B: Block + Hash + Eq> ConsensusContext for PotholeContext<N, B> {
    type ConsensusValue = B;
    type Message = (N, SynchronizerMessage<PotholeDepSpec<B>>);
}

#[derive(Debug)]
pub(crate) struct PotholeWithSynchronizer<N: NodeId, B: Block + Hash + Eq> {
    pothole: PotholeWrapper<B>,
    synchronizer: Synchronizer<N, PotholeWrapper<B>>,
}

impl<N: NodeId, B: Block + Hash + Eq> PotholeWithSynchronizer<N, B> {
    pub(crate) fn new(pothole: Pothole<B>) -> Self {
        Self {
            pothole: PotholeWrapper::new(pothole),
            synchronizer: Synchronizer::new(),
        }
    }
}

fn into_consensus_result<N: NodeId, B: Block + Hash + Eq>(
    pothole_result: PotholeResult<B>,
) -> Option<ConsensusProtocolResult<PotholeContext<N, B>>> {
    match pothole_result {
        PotholeResult::ScheduleTimer(timer_id, instant) => Some(
            ConsensusProtocolResult::ScheduleTimer(instant, TimerId(timer_id)),
        ),
        PotholeResult::CreateNewBlock => Some(ConsensusProtocolResult::CreateNewBlock),
        PotholeResult::FinalizedBlock(_, block) => {
            Some(ConsensusProtocolResult::FinalizedBlock(block))
        }
    }
}

impl<N: NodeId, B: Block + Hash + Eq> ConsensusProtocol<PotholeContext<N, B>>
    for PotholeWithSynchronizer<N, B>
{
    fn handle_message(
        &mut self,
        msg: (N, SynchronizerMessage<PotholeDepSpec<B>>),
    ) -> Result<Vec<ConsensusProtocolResult<PotholeContext<N, B>>>, anyhow::Error> {
        let (sender, msg) = msg;
        Ok(self
            .synchronizer
            .handle_message(&mut self.pothole, sender, msg)
            .into_iter()
            .map(ConsensusProtocolResult::CreatedNewMessage)
            .collect())
    }

    fn handle_timer(
        &mut self,
        timer_id: TimerId,
    ) -> Result<Vec<ConsensusProtocolResult<PotholeContext<N, B>>>, anyhow::Error> {
        Ok(self
            .pothole
            .handle_timer(timer_id.0)
            .into_iter()
            .filter_map(into_consensus_result)
            .collect())
    }
}

impl<B: Block + Hash + Eq + DeserializeOwned> TryFrom<MessageWireFormat>
    for (ConsensusNodeId, SynchronizerMessage<PotholeDepSpec<B>>)
{
    type Error = bincode::Error;

    fn try_from(msg: MessageWireFormat) -> Result<Self, Self::Error> {
        let sync_msg = bincode::deserialize(&msg.message_content)?;
        Ok((msg.sender, sync_msg))
    }
}

impl<B: Block + Hash + Eq + Serialize>
    From<(ConsensusNodeId, SynchronizerMessage<PotholeDepSpec<B>>)> for MessageWireFormat
{
    fn from(
        (node_id, msg): (ConsensusNodeId, SynchronizerMessage<PotholeDepSpec<B>>),
    ) -> MessageWireFormat {
        let message_content = bincode::serialize(&msg).unwrap();
        MessageWireFormat {
            // TODO: include correct EraId here
            era_id: EraId(0),
            sender: node_id,
            message_content,
        }
    }
}
