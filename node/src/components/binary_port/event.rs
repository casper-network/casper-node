use std::fmt::{Display, Formatter};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    Initialize,
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Initialize => write!(f, "initialize"),
        }
    }
}
