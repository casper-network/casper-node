use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::Timestamp;

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::Responder,
    types::{Deploy, DeployHash, FinalizedBlock},
};

#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub(crate) enum ProposableDeploy {
    Transfer(Deploy),
    Deploy(Deploy),
}

impl ProposableDeploy {
    pub(super) fn deploy_hash(&self) -> DeployHash {
        match self {
            ProposableDeploy::Deploy(d) => *d.id(),
            ProposableDeploy::Transfer(t) => *t.id(),
        }
    }
}

pub(crate) enum DeployBufferRequest {
    GetProposableDeploys(Timestamp, Responder<Vec<ProposableDeploy>>),
}

pub(crate) enum Event {
    Initialize,
    Request(DeployBufferRequest),
    ReceiveDeploy(Deploy),
    BlockProposed(Box<ProposedBlock<ClContext>>),
    BlockFinalized(Box<FinalizedBlock>),
    Expire,
}
