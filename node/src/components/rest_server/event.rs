use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    effect::{requests::RestRequest, Responder},
    types::NodeId,
};

use static_assertions::const_assert;
use std::mem;
const _EVENT_SIZE: usize = mem::size_of::<Event2>();
const_assert!(_EVENT_SIZE < 96);

#[derive(Debug, From)]
#[allow(dead_code, clippy::large_enum_variant)]
pub enum Event2 {
    #[from]
    RestRequest(RestRequest<NodeId>),
    GetMetricsResult {
        text: Option<String>,
        main_responder: Responder<Option<String>>,
    },
}

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
