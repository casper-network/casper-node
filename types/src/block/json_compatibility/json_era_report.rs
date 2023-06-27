#[cfg(feature = "datasize")]
use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{super::EraReport, JsonReward};
use crate::PublicKey;

/// A JSON-friendly representation of [`EraReport`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(description = "Equivocation, reward and validator inactivity information.")]
#[serde(deny_unknown_fields)]
pub struct JsonEraReport {
    /// The set of equivocators.
    pub equivocators: Vec<PublicKey>,
    /// Rewards for finalization of earlier blocks.
    pub rewards: Vec<JsonReward>,
    /// Validators that haven't produced any unit during the era.
    pub inactive_validators: Vec<PublicKey>,
}

impl From<EraReport<PublicKey>> for JsonEraReport {
    fn from(era_report: EraReport<PublicKey>) -> Self {
        JsonEraReport {
            equivocators: era_report.equivocators,
            rewards: era_report
                .rewards
                .into_iter()
                .map(|(validator, amount)| JsonReward { validator, amount })
                .collect(),
            inactive_validators: era_report.inactive_validators,
        }
    }
}
