//! This module contains types and functions for managing action thresholds.

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    account::{ActionType, SetThresholdFailure, Weight},
    addressable_entity::WEIGHT_SERIALIZED_LENGTH,
    bytesrepr::{self, Error, FromBytes, ToBytes},
};

/// Thresholds that have to be met when executing an action of a certain type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "json-schema", schemars(rename = "AccountActionThresholds"))]
pub struct ActionThresholds {
    /// Threshold for deploy execution.
    pub deployment: Weight,
    /// Threshold for managing action threshold.
    pub key_management: Weight,
}

impl ActionThresholds {
    /// Creates new ActionThresholds object with provided weights
    ///
    /// Requires deployment threshold to be lower than or equal to
    /// key management threshold.
    pub fn new(
        deployment: Weight,
        key_management: Weight,
    ) -> Result<ActionThresholds, SetThresholdFailure> {
        if deployment > key_management {
            return Err(SetThresholdFailure::DeploymentThreshold);
        }
        Ok(ActionThresholds {
            deployment,
            key_management,
        })
    }
    /// Sets new threshold for [ActionType::Deployment].
    /// Should return an error if setting new threshold for `action_type` breaks
    /// one of the invariants. Currently, invariant is that
    /// `ActionType::Deployment` threshold shouldn't be higher than any
    /// other, which should be checked both when increasing `Deployment`
    /// threshold and decreasing the other.
    pub fn set_deployment_threshold(
        &mut self,
        new_threshold: Weight,
    ) -> Result<(), SetThresholdFailure> {
        if new_threshold > self.key_management {
            Err(SetThresholdFailure::DeploymentThreshold)
        } else {
            self.deployment = new_threshold;
            Ok(())
        }
    }

    /// Sets new threshold for [ActionType::KeyManagement].
    pub fn set_key_management_threshold(
        &mut self,
        new_threshold: Weight,
    ) -> Result<(), SetThresholdFailure> {
        if self.deployment > new_threshold {
            Err(SetThresholdFailure::KeyManagementThreshold)
        } else {
            self.key_management = new_threshold;
            Ok(())
        }
    }

    /// Returns the deployment action threshold.
    pub fn deployment(&self) -> &Weight {
        &self.deployment
    }

    /// Returns key management action threshold.
    pub fn key_management(&self) -> &Weight {
        &self.key_management
    }

    /// Unified function that takes an action type, and changes appropriate
    /// threshold defined by the [ActionType] variants.
    pub fn set_threshold(
        &mut self,
        action_type: ActionType,
        new_threshold: Weight,
    ) -> Result<(), SetThresholdFailure> {
        match action_type {
            ActionType::Deployment => self.set_deployment_threshold(new_threshold),
            ActionType::KeyManagement => self.set_key_management_threshold(new_threshold),
        }
    }
}

impl Default for ActionThresholds {
    fn default() -> Self {
        ActionThresholds {
            deployment: Weight::new(1),
            key_management: Weight::new(1),
        }
    }
}

impl ToBytes for ActionThresholds {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        result.append(&mut self.deployment.to_bytes()?);
        result.append(&mut self.key_management.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        2 * WEIGHT_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.deployment().write_bytes(writer)?;
        self.key_management().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ActionThresholds {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (deployment, rem) = Weight::from_bytes(bytes)?;
        let (key_management, rem) = Weight::from_bytes(rem)?;
        let ret = ActionThresholds {
            deployment,
            key_management,
        };
        Ok((ret, rem))
    }
}

#[doc(hidden)]
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::ActionThresholds;

    pub fn account_action_thresholds_arb() -> impl Strategy<Value = ActionThresholds> {
        Just(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_create_new_action_thresholds() {
        let action_thresholds = ActionThresholds::new(Weight::new(1), Weight::new(42)).unwrap();
        assert_eq!(*action_thresholds.deployment(), Weight::new(1));
        assert_eq!(*action_thresholds.key_management(), Weight::new(42));
    }

    #[test]
    fn should_not_create_action_thresholds_with_invalid_deployment_threshold() {
        // deployment cant be greater than key management
        assert!(ActionThresholds::new(Weight::new(5), Weight::new(1)).is_err());
    }

    #[test]
    fn serialization_roundtrip() {
        let action_thresholds = ActionThresholds::new(Weight::new(1), Weight::new(42)).unwrap();
        bytesrepr::test_serialization_roundtrip(&action_thresholds);
    }
}
