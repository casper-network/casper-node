use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Copy, Clone, PartialEq, Eq, DataSize, Debug, Deserialize, JsonSchema, Serialize)]
pub enum ReactorState {
    // get all components and reactor state set up on start
    Initialize,
    // orient to the network and attempt to catch up to tip
    CatchUp,
    // running commit upgrade and creating immediate switch block
    Upgrading,
    // stay caught up with tip
    KeepUp,
    // node is currently caught up and is an active validator
    Validate,
    // node should be shut down for upgrade
    ShutdownForUpgrade,
}

impl Display for ReactorState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ReactorState::Initialize => write!(f, "Initialize"),
            ReactorState::CatchUp => write!(f, "CatchUp"),
            ReactorState::Upgrading => write!(f, "Upgrading"),
            ReactorState::KeepUp => write!(f, "KeepUp"),
            ReactorState::Validate => write!(f, "Validate"),
            ReactorState::ShutdownForUpgrade => write!(f, "ShutdownForUpgrade"),
        }
    }
}
