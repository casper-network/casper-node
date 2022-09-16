use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::Timestamp;

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::Responder,
    types::{appendable_block::AppendableBlock, Deploy, DeployHash, FinalizedBlock},
};

pub(crate) enum DeployBufferRequest {
    GetAppendableBlock(Timestamp, Responder<AppendableBlock>),
}

pub(crate) enum Event {
    Initialize,
    Request(DeployBufferRequest),
    ReceiveDeploy(Box<Deploy>),
    BlockProposed(Box<ProposedBlock<ClContext>>),
    BlockFinalized(Box<FinalizedBlock>),
    Expire,
}
