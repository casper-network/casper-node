use std::str;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{
    binary_port::ConsensusValidatorChanges, ChainspecRawBytes, EraId, ProtocolVersion, PublicKey,
    ValidatorChange,
};

/// A single change to a validator's status in the given era.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonValidatorStatusChange {
    /// The era in which the change occurred.
    era_id: EraId,
    /// The change in validator status.
    validator_change: ValidatorChange,
}

impl JsonValidatorStatusChange {
    pub(crate) fn new(era_id: EraId, validator_change: ValidatorChange) -> Self {
        JsonValidatorStatusChange {
            era_id,
            validator_change,
        }
    }
}

/// The changes in a validator's status.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonValidatorChanges {
    /// The public key of the validator.
    public_key: PublicKey,
    /// The set of changes to the validator's status.
    status_changes: Vec<JsonValidatorStatusChange>,
}

impl JsonValidatorChanges {
    pub(crate) fn new(
        public_key: PublicKey,
        status_changes: Vec<JsonValidatorStatusChange>,
    ) -> Self {
        JsonValidatorChanges {
            public_key,
            status_changes,
        }
    }
}

/// Result for the "info_get_validator_changes" RPC.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetValidatorChangesResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The validators' status changes.
    pub changes: Vec<JsonValidatorChanges>,
}

impl GetValidatorChangesResult {
    pub(crate) fn new(api_version: ProtocolVersion, changes: ConsensusValidatorChanges) -> Self {
        let changes = changes
            .into_inner()
            .into_iter()
            .map(|(public_key, mut validator_changes)| {
                validator_changes.sort();
                let status_changes = validator_changes
                    .into_iter()
                    .map(|(era_id, validator_change)| {
                        JsonValidatorStatusChange::new(era_id, validator_change)
                    })
                    .collect();
                JsonValidatorChanges::new(public_key, status_changes)
            })
            .collect();
        GetValidatorChangesResult {
            api_version,
            changes,
        }
    }
}

/// Result for the "info_get_chainspec" RPC.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
pub struct GetChainspecResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The chainspec file bytes.
    pub chainspec_bytes: ChainspecRawBytes,
}

impl GetChainspecResult {
    pub(crate) fn new(api_version: ProtocolVersion, chainspec_bytes: ChainspecRawBytes) -> Self {
        Self {
            api_version,
            chainspec_bytes,
        }
    }
}
