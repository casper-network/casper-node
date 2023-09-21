use derive_more::{Display, From};

use crate::{
    components::fetcher::FetchResult,
    effect::requests::BlockValidationRequest,
    types::{Deploy, DeployOrTransferHash},
};

#[derive(Debug, From, Display)]
pub(crate) enum Event {
    #[from]
    Request(BlockValidationRequest),

    #[display(fmt = "{} fetched", dt_hash)]
    DeployFetched {
        dt_hash: DeployOrTransferHash,
        result: FetchResult<Deploy>,
    },
}
