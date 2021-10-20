use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use serde::Serialize;

use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey, U512};

use crate::{
    effect::announcements::LinearChainBlock,
    types::{Block, DeployHash, DeployHeader},
};
use std::collections::BTreeMap;

/// A ContractRuntime announcement.
#[derive(Debug, Serialize)]
pub(crate) enum ContractRuntimeAnnouncement {
    /// A new block from the linear chain was produced.
    LinearChainBlock(#[serde(skip_serializing)] Box<LinearChainBlock>),
    /// A step succeeded and has altered global state.
    StepSuccess {
        /// The era id in which the step was committed to global state.
        era_id: EraId,
        /// The operations and transforms committed to global state.
        execution_effect: ExecutionEffect,
    },
    /// New era validators.
    UpcomingEraValidators {
        /// The era id in which the step was committed to global state.
        era_that_is_ending: EraId,
        /// The validators for the eras after the `era_that_is_ending` era.
        upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    },
}

impl ContractRuntimeAnnouncement {
    /// Creates a `ContractRuntimeAnnouncement::LinearChainBlock` from its parts.
    pub(crate) fn linear_chain_block(
        block: Block,
        execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    ) -> Self {
        Self::LinearChainBlock(Box::new(LinearChainBlock {
            block,
            execution_results,
        }))
    }

    /// Create a ContractRuntimeAnnouncement::StepSuccess from an execution effect.
    pub(crate) fn step_success(era_id: EraId, execution_effect: ExecutionEffect) -> Self {
        Self::StepSuccess {
            era_id,
            execution_effect,
        }
    }
}

impl Display for ContractRuntimeAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeAnnouncement::LinearChainBlock(linear_chain_block) => {
                write!(
                    f,
                    "created linear chain block {}",
                    linear_chain_block.block.hash()
                )
            }
            ContractRuntimeAnnouncement::StepSuccess { era_id, .. } => {
                write!(f, "step completed for {}", era_id)
            }
            ContractRuntimeAnnouncement::UpcomingEraValidators {
                era_that_is_ending, ..
            } => {
                write!(
                    f,
                    "upcoming era validators after current {}.",
                    era_that_is_ending,
                )
            }
        }
    }
}
