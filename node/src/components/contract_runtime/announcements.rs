use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display, Formatter},
};

use casper_execution_engine::core::engine_state::execution_effect::ExecutionEffect as ExecutionEngineExecutionEffect;
use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey};

use crate::{
    effect::{announcements::LinearChainBlock, EffectBuilder},
    types::{Block, DeployHash, DeployHeader},
};

/// A ContractRuntime announcement.
#[derive(Debug)]
pub(crate) enum ContractRuntimeAnnouncement {
    /// A new block from the linear chain was produced.
    LinearChainBlock(Box<LinearChainBlock>),
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
        era_id: EraId,
        /// The validators for the next era (ie, `era_id + 1`).
        next_era_validators: Box<HashSet<PublicKey>>,
        /// The validators for the era after next (ie, `era_id + 2`).
        era_after_next_validators: Box<HashSet<PublicKey>>,
    },
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
                era_id,
                next_era_validators,
                era_after_next_validators,
            } => {
                write!(
                    f,
                    "current era is {current_era}. \
                     validators for era {next_era}: {next_era_validators:?}. \
                     validators for era {era_after_next}: {era_after_next_validators:?}.",
                    current_era = era_id,
                    next_era = *era_id + 1,
                    next_era_validators = next_era_validators,
                    era_after_next = *era_id + 2,
                    era_after_next_validators = era_after_next_validators
                )
            }
        }
    }
}

/// Announce a committed Step success.
pub(super) async fn step_success<REv>(
    effect_builder: EffectBuilder<REv>,
    era_id: EraId,
    execution_effect: ExecutionEngineExecutionEffect,
) where
    REv: From<ContractRuntimeAnnouncement>,
{
    effect_builder
        .schedule_regular(ContractRuntimeAnnouncement::StepSuccess {
            era_id,
            execution_effect: (&execution_effect).into(),
        })
        .await
}

/// Announce new block has been created.
pub(crate) async fn linear_chain_block<REv>(
    effect_builder: EffectBuilder<REv>,
    block: Block,
    execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
) where
    REv: From<ContractRuntimeAnnouncement>,
{
    effect_builder
        .schedule_regular(ContractRuntimeAnnouncement::LinearChainBlock(Box::new(
            LinearChainBlock {
                block,
                execution_results,
            },
        )))
        .await
}

/// Announce validators for upcoming era.
pub(super) async fn upcoming_era_validators<REv>(
    effect_builder: EffectBuilder<REv>,
    era_id: EraId,
    next_era_validators: HashSet<PublicKey>,
    era_after_next_validators: HashSet<PublicKey>,
) where
    REv: From<ContractRuntimeAnnouncement>,
{
    effect_builder
        .schedule_regular(ContractRuntimeAnnouncement::UpcomingEraValidators {
            era_id,
            next_era_validators: Box::new(next_era_validators),
            era_after_next_validators: Box::new(era_after_next_validators),
        })
        .await
}
