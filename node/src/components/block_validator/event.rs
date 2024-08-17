use derive_more::{Display, From};

use casper_types::{EraId, FinalitySignature, FinalitySignatureId, Transaction, TransactionHash};

use crate::{
    components::fetcher::FetchResult, effect::requests::BlockValidationRequest,
    types::BlockWithMetadata,
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

    #[display(fmt = "{} fetched", transaction_hash)]
    TransactionFetched {
        transaction_hash: TransactionHash,
        result: FetchResult<Transaction>,
    },

    #[display(fmt = "{} fetched", finality_signature_id)]
    FinalitySignatureFetched {
        finality_signature_id: Box<FinalitySignatureId>,
        result: FetchResult<FinalitySignature>,
    },

    #[display(fmt = "{} price for era {}", _1, _0)]
    UpdateEraGasPrice(EraId, u8),
}
