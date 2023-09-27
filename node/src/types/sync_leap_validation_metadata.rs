use casper_types::{ActivationPoint, Chainspec, GlobalStateUpdate};
use datasize::DataSize;
use num_rational::Ratio;
use serde::Serialize;

#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SyncLeapValidationMetaData {
    pub(crate) recent_era_count: u64,
    pub(crate) activation_point: ActivationPoint,
    pub(crate) global_state_update: Option<GlobalStateUpdate>,
    #[data_size(skip)]
    pub(crate) finality_threshold_fraction: Ratio<u64>,
}

impl SyncLeapValidationMetaData {
    #[cfg(test)]
    pub fn new(
        recent_era_count: u64,
        activation_point: ActivationPoint,
        global_state_update: Option<GlobalStateUpdate>,
        finality_threshold_fraction: Ratio<u64>,
    ) -> Self {
        Self {
            recent_era_count,
            activation_point,
            global_state_update,
            finality_threshold_fraction,
        }
    }

    pub(crate) fn from_chainspec(chainspec: &Chainspec) -> Self {
        Self {
            recent_era_count: chainspec.core_config.recent_era_count(),
            activation_point: chainspec.protocol_config.activation_point,
            global_state_update: chainspec.protocol_config.global_state_update.clone(),
            finality_threshold_fraction: chainspec.core_config.finality_threshold_fraction,
        }
    }
}
