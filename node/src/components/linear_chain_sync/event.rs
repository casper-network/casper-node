use std::fmt::{Debug, Display};

use crate::types::BlockHeader;

#[derive(Debug)]
pub enum Event {
    Finish(Box<BlockHeader>),
    Start,
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Finish(block) => write!(f, "Finished syncing, final block header: {}.", block),
            Event::Start => write!(f, "Starting fast sync"),
        }
    }
}
