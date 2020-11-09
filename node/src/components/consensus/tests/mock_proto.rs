use std::{
    any::Any,
    collections::{BTreeSet, HashSet, VecDeque},
};

use casper_execution_engine::shared::motes::Motes;
use datasize::DataSize;
use derive_more::Display;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    components::{
        chainspec_loader::Chainspec,
        consensus::{
            consensus_protocol::{
                BlockContext, ConsensusProtocol, ConsensusProtocolResult as CpResult,
                FinalizedBlock,
            },
            traits::Context,
            ConsensusMessage, EraId, Event,
        },
    },
    types::Timestamp,
    NodeRng,
};

#[derive(DataSize, Debug, Ord, PartialOrd, Clone, Display, Hash, Eq, PartialEq)]
pub(crate) struct NodeId(pub u8);

/// A mock network message.
/// These are basically instructions to the `MockProto` to trigger certain results.
#[derive(Serialize, Deserialize, Debug)]
#[serde(bound(
    serialize = "C::ValidatorId: Serialize",
    deserialize = "C::ValidatorId: Deserialize<'de>",
))]
pub(crate) enum Message<C: Context> {
    /// Evidence that a validator has equivocated.
    Evidence(C::ValidatorId),
    /// A block proposed by another validator.
    BlockByOtherValidator {
        value: C::ConsensusValue,
        timestamp: Timestamp,
        proposer: C::ValidatorId,
    },
    /// Represents network messages that cause the next block to be finalized.
    FinalizeBlock,
}

impl<C: Context> Message<C>
where
    C::ValidatorId: Serialize,
{
    /// Creates a `MessageReceived` event from this message.
    pub(crate) fn received(&self, sender: NodeId, era_id: EraId) -> Event<NodeId> {
        let payload = bincode::serialize(self).expect("should serialize message");
        let msg = ConsensusMessage::Protocol { era_id, payload };
        Event::MessageReceived { sender, msg }
    }
}

/// A block that was proposed but not finalized yet.
#[derive(DataSize, Debug)]
pub(crate) struct PendingBlock<C>
where
    C: Context,
{
    value: C::ConsensusValue,
    timestamp: Timestamp,
    proposer: C::ValidatorId,
}

/// A mock consensus protocol for testing.
#[derive(DataSize, Debug)]
pub(crate) struct MockProto<C>
where
    C: Context,
{
    /// The unique ID of this protocol instance.
    instance_id: C::InstanceId,
    /// Validators that were observed to be faulty.
    evidence: BTreeSet<C::ValidatorId>,
    /// Validators marked as faulty due to external evidence.
    faulty: BTreeSet<C::ValidatorId>,
    /// This validator's ID and secret key, if active.
    active_validator: Option<(C::ValidatorId, C::ValidatorSecret)>,
    pending_blocks: VecDeque<PendingBlock<C>>,
    finalized_blocks: Vec<FinalizedBlock<C>>,
}

impl<C: Context + 'static> MockProto<C>
where
    C::ValidatorId: Serialize + DeserializeOwned,
{
    /// Creates a new boxed `MockProto` instance.
    pub(crate) fn new_boxed(
        instance_id: C::InstanceId,
        _validator_stakes: Vec<(C::ValidatorId, Motes)>,
        _slashed: &HashSet<C::ValidatorId>,
        _chainspec: &Chainspec,
        _prev_cp: Option<&dyn ConsensusProtocol<NodeId, C>>,
        _start_time: Timestamp,
        _seed: u64,
    ) -> Box<dyn ConsensusProtocol<NodeId, C>> {
        Box::new(MockProto {
            instance_id,
            evidence: Default::default(),
            faulty: Default::default(),
            active_validator: None,
            pending_blocks: Default::default(),
            finalized_blocks: Default::default(),
        })
    }
}

impl<C: Context + 'static> ConsensusProtocol<NodeId, C> for MockProto<C>
where
    C::ValidatorId: Serialize + DeserializeOwned,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn handle_message(
        &mut self,
        sender: NodeId,
        msg: Vec<u8>,
        evidence_only: bool,
        _rng: &mut NodeRng,
    ) -> Vec<CpResult<NodeId, C>> {
        match bincode::deserialize::<Message<C>>(msg.as_slice()) {
            Err(err) => vec![CpResult::InvalidIncomingMessage(msg, sender, err.into())],
            Ok(Message::Evidence(vid)) => {
                if self.evidence.insert(vid.clone()) {
                    let msg = Message::Evidence::<C>(vid);
                    vec![CpResult::CreatedGossipMessage(
                        bincode::serialize(&msg).expect("should serialize message"),
                    )]
                } else {
                    vec![]
                }
            }
            Ok(Message::FinalizeBlock) | Ok(Message::BlockByOtherValidator { .. })
                if evidence_only =>
            {
                vec![] // Evidence only: Other incoming messages are ignored.
            }
            Ok(Message::BlockByOtherValidator {
                value,
                timestamp,
                proposer,
            }) => {
                let result = CpResult::ValidateConsensusValue(sender, value.clone(), timestamp);
                self.pending_blocks.push_back(PendingBlock {
                    value,
                    timestamp,
                    proposer,
                });
                vec![result]
            }
            Ok(Message::FinalizeBlock) => {
                let PendingBlock {
                    value,
                    timestamp,
                    proposer,
                } = self
                    .pending_blocks
                    .pop_front()
                    .expect("should have pending blocks when handling FinalizeBlock");
                let fb = FinalizedBlock {
                    value,
                    timestamp,
                    height: self.finalized_blocks.len() as u64,
                    rewards: Default::default(),
                    equivocators: Default::default(),
                    proposer,
                };
                self.finalized_blocks.push(fb.clone());
                let result = CpResult::FinalizedBlock(fb);
                vec![result]
            }
        }
    }

    fn handle_timer(
        &mut self,
        _timestamp: Timestamp,
        _rng: &mut NodeRng,
    ) -> Vec<CpResult<NodeId, C>> {
        todo!("implement handle_timer")
    }

    fn propose(
        &mut self,
        _value: C::ConsensusValue,
        _block_context: BlockContext,
        _rng: &mut NodeRng,
    ) -> Vec<CpResult<NodeId, C>> {
        todo!("implement propose")
    }

    fn resolve_validity(
        &mut self,
        _value: &C::ConsensusValue,
        _valid: bool,
        _rng: &mut NodeRng,
    ) -> Vec<CpResult<NodeId, C>> {
        todo!("implement resolve_validity")
    }

    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        _timestamp: Timestamp,
    ) -> Vec<CpResult<NodeId, C>> {
        self.active_validator = Some((our_id, secret));
        vec![]
    }

    fn deactivate_validator(&mut self) {
        self.active_validator = None;
    }

    fn has_evidence(&self, vid: &C::ValidatorId) -> bool {
        self.evidence.contains(vid)
    }

    fn mark_faulty(&mut self, vid: &C::ValidatorId) {
        if !self.evidence.contains(vid) {
            self.faulty.insert(vid.clone());
        }
    }

    fn request_evidence(&self, sender: NodeId, vid: &C::ValidatorId) -> Vec<CpResult<NodeId, C>> {
        if self.evidence.contains(vid) {
            let msg = Message::<C>::Evidence(vid.clone());
            let ser_msg = bincode::serialize(&msg).expect("should serialize message");
            let result = CpResult::CreatedTargetedMessage(ser_msg, sender);
            vec![result]
        } else {
            vec![]
        }
    }

    fn validators_with_evidence(&self) -> Vec<&C::ValidatorId> {
        self.evidence.iter().collect()
    }

    fn has_received_messages(&self) -> bool {
        !self.pending_blocks.is_empty() || !self.finalized_blocks.is_empty()
    }
}
