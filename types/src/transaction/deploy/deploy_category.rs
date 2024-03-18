use core::fmt::{self, Formatter};

use crate::Deploy;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The category of a [`Transaction`].
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Session kind of legacy Deploy.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum DeployCategory {
    /// Standard transaction (the default).
    #[default]
    Standard = 0,
    /// Native transfer interaction.
    Transfer = 1,
}

impl fmt::Display for DeployCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DeployCategory::Standard => write!(f, "Standard"),
            DeployCategory::Transfer => write!(f, "Transfer"),
        }
    }
}

impl From<Deploy> for DeployCategory {
    fn from(value: Deploy) -> Self {
        if value.is_transfer() {
            DeployCategory::Transfer
        } else {
            DeployCategory::Standard
        }
    }
}
