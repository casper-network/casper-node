use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
};

use datasize::DataSize;
use itertools::Itertools;
use once_cell::sync::Lazy;
use tracing::{debug, warn};

use casper_types::{PublicKey, U512};

use crate::{
    components::consensus::{
        cl_context::ClContext,
        consensus_protocol::{ConsensusProtocol, ProposedBlock},
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

pub struct Era {
    /// The consensus protocol instance.
    pub(crate) consensus: Box<dyn ConsensusProtocol<ClContext>>,
    /// The scheduled starting time of this era.
    pub(crate) start_time: Timestamp,
    /// The height of this era's first block.
    pub(crate) start_height: u64,
    /// Pending blocks, waiting for validation and dependencies.
    pub(crate) validation_states: HashMap<ProposedBlock<ClContext>, ValidationState>,
    /// Validators banned in this and the next BONDED_ERAS eras, because they were faulty in the
    /// previous switch block.
    pub(crate) faulty: HashSet<PublicKey>,
    /// Validators that are excluded from proposing new blocks.
    pub(crate) cannot_propose: HashSet<PublicKey>,
    /// Accusations collected in this era so far.
    pub(crate) accusations: HashSet<PublicKey>,
    /// The validator weights.
    pub(crate) validators: BTreeMap<PublicKey, U512>,
}

impl Era {
    pub(crate) fn new(
        consensus: Box<dyn ConsensusProtocol<ClContext>>,
        start_time: Timestamp,
        start_height: u64,
        faulty: HashSet<PublicKey>,
        cannot_propose: HashSet<PublicKey>,
        validators: BTreeMap<PublicKey, U512>,
    ) -> Self {
        Era {
            consensus,
            start_time,
            start_height,
            validation_states: HashMap::new(),
            faulty,
            cannot_propose,
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
}

impl DataSize for Era {
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
            faulty,
            cannot_propose,
            accusations,
            validators,
        } = self;

        // `DataSize` cannot be made object safe due its use of associated constants. We implement
        // it manually here, downcasting the consensus protocol as a workaround.

        let consensus_heap_size = {
            let any_ref = consensus.as_any();

            if let Some(highway) = any_ref.downcast_ref::<HighwayProtocol<ClContext>>() {
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
                    HighwayProtocol<ClContext> to determine heap allocation size"
                );
                0
            }
        };

        consensus_heap_size
            .saturating_add(start_time.estimate_heap_size())
            .saturating_add(start_height.estimate_heap_size())
            .saturating_add(validation_states.estimate_heap_size())
            .saturating_add(faulty.estimate_heap_size())
            .saturating_add(cannot_propose.estimate_heap_size())
            .saturating_add(accusations.estimate_heap_size())
            .saturating_add(validators.estimate_heap_size())
    }
}
