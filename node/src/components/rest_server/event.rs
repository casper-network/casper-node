use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    components::small_network::NodeId,
    effect::{requests::RestRequest, Responder},
    types::{
        json_compatibility::ExecutionResult, BlockHash, BlockHeader, DeployHash, DeployHeader,
        FinalizedBlock,
    },
};

#[derive(Debug, From)]
pub enum Event {
    #[from]
    RestRequest(RestRequest<NodeId>),
    GetMetricsResult {
        text: Option<String>,
        main_responder: Responder<Option<String>>,
    },
    BlockFinalized(Box<FinalizedBlock>),
    BlockAdded {
        block_hash: BlockHash,
        block_header: Box<BlockHeader>,
    },
    DeployProcessed {
        deploy_hash: DeployHash,
        deploy_header: Box<DeployHeader>,
        block_hash: BlockHash,
        execution_result: Box<ExecutionResult>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::RestRequest(request) => write!(formatter, "{}", request),
            Event::GetMetricsResult { text, .. } => match text {
                Some(txt) => write!(formatter, "get metrics ({} bytes)", txt.len()),
                None => write!(formatter, "get metrics (failed)"),
            },
            Event::BlockFinalized(finalized_block) => write!(
                formatter,
                "block finalized {}",
                finalized_block.proto_block().hash()
            ),
            Event::BlockAdded { block_hash, .. } => write!(formatter, "block added {}", block_hash),
            Event::DeployProcessed { deploy_hash, .. } => {
                write!(formatter, "deploy processed {}", deploy_hash)
            }
        }
    }
}
