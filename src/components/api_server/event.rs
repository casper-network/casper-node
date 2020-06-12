use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    components::storage,
    effect::{requests::ApiRequest, Responder},
    types::{Deploy, DeployHash},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    #[from]
    ApiRequest(ApiRequest),
    PutDeployResult {
        deploy: Box<Deploy>,
        result: storage::Result<()>,
        main_responder: Responder<Result<(), (Deploy, String)>>,
    },
    GetDeployResult {
        hash: DeployHash,
        result: Box<storage::Result<Deploy>>,
        main_responder: Responder<Option<Deploy>>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::ApiRequest(request) => write!(formatter, "{}", request),
            Event::PutDeployResult { result, .. } => {
                write!(formatter, "PutDeployResult: {:?}", result)
            }
            Event::GetDeployResult { hash, result, .. } => {
                write!(formatter, "GetDeployResult for {}: {:?}", hash, result)
            }
        }
    }
}
