use derive_more::{Display, From};

use crate::{
    components::fetcher::FetchResult,
    effect::requests::ProposedBlockValidationRequest,
    types::{Deploy, DeployOrTransferHash},
};

#[derive(Debug, From, Display)]
pub(crate) enum Event {
    #[from]
    Request(ProposedBlockValidationRequest),

    #[display(fmt = "{} fetched", dt_hash)]
    DeployFetched {
        dt_hash: DeployOrTransferHash,
        result: FetchResult<Deploy>,
    },
}
