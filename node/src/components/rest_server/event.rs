use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    components::small_network::NodeId,
    effect::{requests::RestRequest, Responder},
};

#[derive(Debug, From)]
pub enum Event {
    #[from]
    RestRequest(RestRequest<NodeId>),
    GetMetricsResult {
        text: Option<String>,
        main_responder: Responder<Option<String>>,
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
        }
    }
}
