use derive_more::{Display, From};

use casper_types::Transaction;

use crate::{
    components::fetcher::FetchResult, effect::requests::BlockValidationRequest,
    types::DeployOrTransferHash,
};

#[derive(Debug, From, Display)]
pub(crate) enum Event {
    #[from]
    Request(BlockValidationRequest),

    #[display(fmt = "{} fetched", dt_hash)]
    TransactionFetched {
        dt_hash: DeployOrTransferHash,
        result: FetchResult<Transaction>,
    },
}
