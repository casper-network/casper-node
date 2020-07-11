use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    components::storage,
    effect::{requests::ApiRequest, Responder},
    types::{Deploy, DeployHash},
};

#[derive(Debug, From)]
pub enum Event {
    #[from]
    ApiRequest(ApiRequest),
    PutDeployResult {
        deploy: Box<Deploy>,
        result: storage::Result<()>,
        main_responder: Responder<Result<(), (Deploy, storage::Error)>>,
    },
    GetDeployResult {
        hash: DeployHash,
        result: Box<storage::Result<Deploy>>,
        main_responder: Responder<storage::Result<Deploy>>,
    },
    ListDeploysResult {
        result: Box<storage::Result<Vec<DeployHash>>>,
        main_responder: Responder<storage::Result<Vec<DeployHash>>>,
    },
    GetMetricsResult {
        text: Option<String>,
        main_responder: Responder<Option<String>>,
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
            Event::ListDeploysResult { result, .. } => {
                write!(formatter, "ListDeployResult: {:?}", result)
            }
            Event::GetMetricsResult { text, .. } => match text {
                Some(tx) => write!(formatter, "GetMetricsResult ({} bytes)", tx.len()),
                None => write!(formatter, "GetMetricsResult (failed)"),
            },
        }
    }
}
