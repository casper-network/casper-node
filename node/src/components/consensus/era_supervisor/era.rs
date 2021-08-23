use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
};

use datasize::DataSize;
use itertools::Itertools;
use num_traits::AsPrimitive;
use once_cell::sync::Lazy;
use tracing::{debug, error, warn};

use casper_types::{system::auction::BLOCK_REWARD, PublicKey, U512};

use crate::{
    components::consensus::{
        cl_context::ClContext,
        config::ProtocolConfig,
        consensus_protocol::{ConsensusProtocol, EraReport, ProposedBlock, TerminalBlockData},
        protocols::highway::HighwayProtocol,
    },
    types::Timestamp,
};

const CASPER_ENABLE_DETAILED_CONSENSUS_METRICS_ENV_VAR: &str =
    "CASPER_ENABLE_DETAILED_CONSENSUS_METRICS";
static CASPER_ENABLE_DETAILED_CONSENSUS_METRICS: Lazy<bool> =
    Lazy::new(|| env::var(CASPER_ENABLE_DETAILED_CONSENSUS_METRICS_ENV_VAR).is_ok());

/// A proposed block waiting for validation and dependencies.
#[derive(DataSize)]
pub struct ValidationState {
    /// Whether the block has been validated yet.
    validated: bool,
    /// A list of IDs of accused validators for which we are still missing evidence.
    missing_evidence: Vec<PublicKey>,
}

impl ValidationState {
    fn new(missing_evidence: Vec<PublicKey>) -> Self {
        ValidationState {
            validated: false,
            missing_evidence,
        }
    }

    fn is_complete(&self) -> bool {
        self.validated && self.missing_evidence.is_empty()
    }
}

pub struct Era<I> {
    /// The consensus protocol instance.
    pub(crate) consensus: Box<dyn ConsensusProtocol<I, ClContext>>,
    /// The scheduled starting time of this era.
    pub(crate) start_time: Timestamp,
    /// The height of this era's first block.
    pub(crate) start_height: u64,
    /// Pending blocks, waiting for validation and dependencies.
    validation_states: HashMap<ProposedBlock<ClContext>, ValidationState>,
    /// Validators banned in this and the next BONDED_ERAS eras, because they were faulty in the
    /// previous switch block.
    pub(crate) new_faulty: Vec<PublicKey>,
    /// Validators that have been faulty in any of the recent BONDED_ERAS switch blocks. This
    /// includes `new_faulty`.
    pub(crate) faulty: HashSet<PublicKey>,
    /// Accusations collected in this era so far.
    accusations: HashSet<PublicKey>,
    /// The validator weights.
    validators: BTreeMap<PublicKey, U512>,
}

impl<I> Era<I> {
    pub(crate) fn new(
        consensus: Box<dyn ConsensusProtocol<I, ClContext>>,
        start_time: Timestamp,
        start_height: u64,
        new_faulty: Vec<PublicKey>,
        faulty: HashSet<PublicKey>,
        validators: BTreeMap<PublicKey, U512>,
    ) -> Self {
        Era {
            consensus,
            start_time,
            start_height,
            validation_states: HashMap::new(),
            new_faulty,
            faulty,
            accusations: HashSet::new(),
            validators,
        }
    }

    /// Adds a new block, together with the accusations for which we don't have evidence yet.
    pub(crate) fn add_block(
        &mut self,
        proposed_block: ProposedBlock<ClContext>,
        missing_evidence: Vec<PublicKey>,
    ) {
        self.validation_states
            .insert(proposed_block, ValidationState::new(missing_evidence));
    }

    /// Marks the dependencies of blocks on evidence against validator `pub_key` as resolved and
    /// returns all valid blocks that have no missing dependencies left.
    pub(crate) fn resolve_evidence_and_mark_faulty(
        &mut self,
        pub_key: &PublicKey,
    ) -> Vec<ProposedBlock<ClContext>> {
        for pc in self.validation_states.values_mut() {
            pc.missing_evidence.retain(|pk| pk != pub_key);
        }
        self.consensus.mark_faulty(pub_key);
        let (complete, incomplete): (HashMap<_, _>, HashMap<_, _>) = self
            .validation_states
            .drain()
            .partition(|(_, validation_state)| validation_state.is_complete());
        self.validation_states = incomplete;
        complete
            .into_iter()
            .map(|(proposed_block, _)| proposed_block)
            .collect()
    }

    /// Marks the block payload as valid or invalid. Returns `false` if the block was not present
    /// or is still missing evidence. Otherwise it returns `true`: The block can now be processed
    /// by the consensus protocol.
    pub(crate) fn resolve_validity(
        &mut self,
        proposed_block: &ProposedBlock<ClContext>,
        valid: bool,
    ) -> bool {
        if valid {
            if let Some(vs) = self.validation_states.get_mut(proposed_block) {
                if !vs.missing_evidence.is_empty() {
                    vs.validated = true;
                    return false;
                }
            }
        }
        self.validation_states.remove(proposed_block).is_some()
    }

    /// Adds new accusations from a finalized block.
    pub(crate) fn add_accusations(&mut self, accusations: &[PublicKey]) {
        for pub_key in accusations {
            if !self.faulty.contains(pub_key) {
                self.accusations.insert(pub_key.clone());
            }
        }
    }

    /// Returns all accusations from finalized blocks so far.
    pub(crate) fn accusations(&self) -> Vec<PublicKey> {
        self.accusations.iter().cloned().sorted().collect()
    }

    /// Returns the map of validator weights.
    pub(crate) fn validators(&self) -> &BTreeMap<PublicKey, U512> {
        &self.validators
    }

    /// Sets the pause status: While paused we don't create consensus messages other than pings.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.consensus.set_paused(paused);
    }

    /// Computes rewards and returns the era report.
    pub(crate) fn create_report(
        &self,
        terminal_block_data: TerminalBlockData<ClContext>,
        timestamp: Timestamp,
        protocol_config: &ProtocolConfig,
    ) -> EraReport<PublicKey> {
        let TerminalBlockData {
            inactive_validators,
        } = terminal_block_data;

        // The total rewards are the ideal number of blocks (i.e. one per minimum-length round) in
        // the actual duration of the era, times the BLOCK_REWARD constant, independent of how many
        // blocks were actually created.
        let era_duration = timestamp.saturating_diff(self.start_time);
        let min_round_length = protocol_config.highway_config.min_round_length();
        let number_of_min_length_rounds = era_duration / min_round_length;
        let total_rewards: u64 = number_of_min_length_rounds
            .checked_mul(BLOCK_REWARD)
            .unwrap_or_else(|| {
                // With BLOCK_REWARD set to one trillion and a round length of 1 minute, a single
                // era would have to last 35 years before the total rewards would overflow a u64.
                error!(%era_duration, %min_round_length, %BLOCK_REWARD, "total rewards overflow");
                u64::MAX
            });

        if total_rewards == 0 {
            return EraReport {
                rewards: BTreeMap::new(),
                equivocators: self.accusations(),
                inactive_validators,
            };
        }

        let is_active = |public_key: &PublicKey| {
            !(self.accusations.contains(public_key)
                || self.faulty.contains(public_key)
                || inactive_validators.contains(public_key))
        };

        // The total rewards are distributed proportionally by weight. To make sure that the
        // computation doesn't overflow we divide all weights by 2^64 if the numbers are too large.
        let total_weight: U512 = self
            .validators
            .iter()
            .filter_map(|(public_key, weight)| is_active(public_key).then(|| *weight))
            .sum();
        let shift = if total_weight.checked_mul(total_rewards.into()).is_none() {
            64
        } else {
            0
        };
        let rewards = self
            .validators
            .iter()
            .filter_map(|(public_key, weight)| {
                is_active(public_key).then(|| {
                    let reward = (weight >> shift) * total_rewards / (total_weight >> shift);
                    (public_key.clone(), AsPrimitive::as_(reward))
                })
            })
            .collect();
        EraReport {
            rewards,
            equivocators: self.accusations(),
            inactive_validators,
        }
    }
}

impl<I> DataSize for Era<I>
where
    I: DataSize + 'static,
{
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    #[inline]
    fn estimate_heap_size(&self) -> usize {
        // Destructure self, so we can't miss any fields.
        let Era {
            consensus,
            start_time,
            start_height,
            validation_states,
            new_faulty,
            faulty,
            accusations,
            validators,
        } = self;

        // `DataSize` cannot be made object safe due its use of associated constants. We implement
        // it manually here, downcasting the consensus protocol as a workaround.

        let consensus_heap_size = {
            let any_ref = consensus.as_any();

            if let Some(highway) = any_ref.downcast_ref::<HighwayProtocol<I, ClContext>>() {
                if *CASPER_ENABLE_DETAILED_CONSENSUS_METRICS {
                    let detailed = (*highway).estimate_detailed_heap_size();
                    match serde_json::to_string(&detailed) {
                        Ok(encoded) => debug!(%encoded, "consensus memory metrics"),
                        Err(err) => warn!(%err, "error encoding consensus memory metrics"),
                    }
                    detailed.total()
                } else {
                    (*highway).estimate_heap_size()
                }
            } else {
                warn!(
                    "could not downcast consensus protocol to \
                    HighwayProtocol<I, ClContext> to determine heap allocation size"
                );
                0
            }
        };

        consensus_heap_size
            .saturating_add(start_time.estimate_heap_size())
            .saturating_add(start_height.estimate_heap_size())
            .saturating_add(validation_states.estimate_heap_size())
            .saturating_add(new_faulty.estimate_heap_size())
            .saturating_add(faulty.estimate_heap_size())
            .saturating_add(accusations.estimate_heap_size())
            .saturating_add(validators.estimate_heap_size())
    }
}

#[cfg(test)]
mod tests {
    use crate::components::consensus::{
        protocols::highway::tests::new_test_highway_protocol, tests::utils::*,
    };

    use super::*;

    #[test]
    fn create_report() {
        const ALICE_STAKE: u64 = 40;
        const BOB_STAKE: u64 = 40;
        const CAROL_STAKE: u64 = 40;
        const DAN_STAKE: u64 = 40;
        const ERIC_STAKE: u64 = 40;
        let validators: BTreeMap<PublicKey, U512> = vec![
            (ALICE_PUBLIC_KEY.clone(), ALICE_STAKE.into()),
            (BOB_PUBLIC_KEY.clone(), BOB_STAKE.into()),
            (CAROL_PUBLIC_KEY.clone(), CAROL_STAKE.into()),
            (DAN_PUBLIC_KEY.clone(), DAN_STAKE.into()),
            (ERIC_PUBLIC_KEY.clone(), ERIC_STAKE.into()),
        ]
        .into_iter()
        .collect();
        let start_time = Timestamp::from(0);
        let chainspec = new_test_chainspec(validators.clone());
        let protocol_config = ProtocolConfig::from(&chainspec);
        let round_len = chainspec.highway_config.min_round_length();
        let new_faulty = vec![DAN_PUBLIC_KEY.clone()];
        let faulty: HashSet<PublicKey> = vec![DAN_PUBLIC_KEY.clone(), ERIC_PUBLIC_KEY.clone()]
            .into_iter()
            .collect();
        let mut era = Era::new(
            new_test_highway_protocol(validators.clone(), faulty.iter().cloned()),
            start_time,
            0,
            new_faulty,
            faulty,
            validators,
        );

        let number_of_rounds_in_this_era = 4;
        let time = start_time + round_len * number_of_rounds_in_this_era;

        // The era lasted four rounds, so the total reward is 4 * BLOCK_REWARD.
        // Dan and Eric recently equivocated, so they don't get a reward.
        let tbd = TerminalBlockData {
            inactive_validators: vec![],
        };
        let report = {
            let total = ALICE_STAKE + BOB_STAKE + CAROL_STAKE;
            EraReport {
                rewards: vec![
                    (
                        ALICE_PUBLIC_KEY.clone(),
                        ALICE_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                    (
                        BOB_PUBLIC_KEY.clone(),
                        BOB_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                    (
                        CAROL_PUBLIC_KEY.clone(),
                        CAROL_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                ]
                .into_iter()
                .collect(),
                equivocators: vec![],
                inactive_validators: vec![],
            }
        };
        assert_eq!(report, era.create_report(tbd, time, &protocol_config));

        // If Carol is inactive in this era, she doesn't get a reward.
        let tbd = TerminalBlockData {
            inactive_validators: vec![CAROL_PUBLIC_KEY.clone()],
        };
        let report = {
            let total = ALICE_STAKE + BOB_STAKE;
            EraReport {
                rewards: vec![
                    (
                        ALICE_PUBLIC_KEY.clone(),
                        ALICE_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                    (
                        BOB_PUBLIC_KEY.clone(),
                        BOB_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                ]
                .into_iter()
                .collect(),
                equivocators: vec![],
                inactive_validators: vec![CAROL_PUBLIC_KEY.clone()],
            }
        };
        assert_eq!(report, era.create_report(tbd, time, &protocol_config));

        // And if she is faulty, she doesn't get one either.
        let tbd = TerminalBlockData {
            inactive_validators: vec![],
        };
        era.add_accusations(&[CAROL_PUBLIC_KEY.clone()]);
        let report = {
            let total = ALICE_STAKE + BOB_STAKE;
            EraReport {
                rewards: vec![
                    (
                        ALICE_PUBLIC_KEY.clone(),
                        ALICE_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                    (
                        BOB_PUBLIC_KEY.clone(),
                        BOB_STAKE * number_of_rounds_in_this_era * BLOCK_REWARD / total,
                    ),
                ]
                .into_iter()
                .collect(),
                equivocators: vec![CAROL_PUBLIC_KEY.clone()],
                inactive_validators: vec![],
            }
        };
        assert_eq!(report, era.create_report(tbd, time, &protocol_config));

        // Finally, if the era had length 0, the rewards will be empty.
        let time = start_time;
        let tbd = TerminalBlockData {
            inactive_validators: vec![],
        };
        let report = {
            EraReport {
                rewards: BTreeMap::new(),
                equivocators: vec![CAROL_PUBLIC_KEY.clone()],
                inactive_validators: vec![],
            }
        };
        assert_eq!(report, era.create_report(tbd, time, &protocol_config));
    }
}
