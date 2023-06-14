#[cfg(feature = "datasize")]
use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    super::{EraEnd, EraReport},
    JsonEraReport, JsonValidatorWeight,
};

/// A JSON-friendly representation of [`EraEnd`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(
    description = "Information related to the end of an era, and validator weights for the \
    following era."
)]
#[serde(deny_unknown_fields)]
pub struct JsonEraEnd {
    /// Equivocation, reward and validator inactivity information.
    pub era_report: JsonEraReport,
    /// The validators for the upcoming era and their respective weights.
    pub next_era_validator_weights: Vec<JsonValidatorWeight>,
}

impl From<EraEnd> for JsonEraEnd {
    fn from(data: EraEnd) -> Self {
        let json_era_end = JsonEraReport::from(data.era_report);
        let json_validator_weights = data
            .next_era_validator_weights
            .iter()
            .map(|(validator, weight)| JsonValidatorWeight {
                validator: validator.clone(),
                weight: *weight,
            })
            .collect();
        JsonEraEnd {
            era_report: json_era_end,
            next_era_validator_weights: json_validator_weights,
        }
    }
}

impl From<JsonEraEnd> for EraEnd {
    fn from(json_data: JsonEraEnd) -> Self {
        let era_report = EraReport::from(json_data.era_report);
        let validator_weights = json_data
            .next_era_validator_weights
            .iter()
            .map(|validator_weight| (validator_weight.validator.clone(), validator_weight.weight))
            .collect();
        EraEnd::new(era_report, validator_weights)
    }
}
