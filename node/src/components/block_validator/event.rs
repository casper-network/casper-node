use derive_more::{Display, From};

use casper_types::{FinalitySignature, FinalitySignatureId, Transaction};

use crate::{
    components::fetcher::FetchResult,
    effect::requests::BlockValidationRequest,
    types::{BlockWithMetadata, DeployOrTransferHash},
};

#[derive(Debug, From, Display)]
pub(crate) enum Event {
    #[from]
    Request(BlockValidationRequest),

    #[display(fmt = "past blocks read from storage")]
    GotPastBlocksWithMetadata {
        past_blocks_with_metadata: Vec<Option<BlockWithMetadata>>,
        request: BlockValidationRequest,
    },

    #[display(fmt = "block {} has been stored", _0)]
    BlockStored(u64),

    #[display(fmt = "{} fetched", dt_hash)]
    TransactionFetched {
        dt_hash: DeployOrTransferHash,
        result: FetchResult<Transaction>,
    },

    #[display(fmt = "{} fetched", finality_signature_id)]
    FinalitySignatureFetched {
        finality_signature_id: Box<FinalitySignatureId>,
        result: FetchResult<FinalitySignature>,
    },
}
