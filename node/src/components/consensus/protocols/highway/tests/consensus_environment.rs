// This is a test module, so we're not worried about integer arithmetic here.
#![allow(clippy::integer_arithmetic)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
};

use casper_types::{PublicKey, Timestamp};

use crate::{
    components::consensus::{
        cl_context::{ClContext, Keypair},
        consensus_protocol::{ConsensusProtocol, FinalizedBlock, ProtocolOutcome},
        era_supervisor::debug::EraDump,
        highway_core::{
            highway::{SignedWireUnit, Vertex, WireUnit},
            State,
        },
        protocols::highway::{
            HighwayMessage, HighwayProtocol, ACTION_ID_VERTEX, TIMER_ID_ACTIVE_VALIDATOR,
        },
        tests::utils::ALICE_NODE_ID,
        traits::Context,
        utils::ValidatorIndex,
        LeaderSequence, ProposedBlock, SerializedMessage,
    },
    NodeRng,
};

use super::new_test_highway_protocol_with_era_height;

/// An environment that simulates a single instance of Highway and how it processes consensus
/// messages.
/// The consensus instance is assumed to be the one held by the validator at index 0. The other
/// validators exist only as keypairs, and their activity is simulated by means of passing
/// SignedWireUnits to the Highway instance. Some validators can be defined to be slow, which means
/// they generate their units before they can receive units from other nodes, but the states are
/// assumed to be consistent at the end of each consensus round.
/// Since the only protocol state in this simulation is the one in the Highway instance of node 0,
/// it is also used as the basis for generation of the simulated consensus messages sent from other
/// nodes.
pub(super) struct ConsensusEnvironment {
    highway: Box<dyn ConsensusProtocol<ClContext>>,
    leaders: LeaderSequence,
    validators: BTreeMap<PublicKey, (Keypair, u64)>,
    current_round_start: u64,
    slow_validators: BTreeSet<PublicKey>,
    rng: NodeRng,
    finalized_blocks: Vec<FinalizedBlock<ClContext>>,
}

impl ConsensusEnvironment {
    /// Creates a new `ConsensusEnvironment` with the given set of validators and slow validators.
    /// The public keys in `slow_validators` should exist in the `validators` map and have their
    /// associated keypairs and weights.
    pub(super) fn new(
        validators: BTreeMap<PublicKey, (Keypair, u64)>,
        slow_validators: BTreeSet<PublicKey>,
    ) -> Self {
        let mut highway = new_test_highway_protocol_with_era_height(
            validators
                .iter()
                .map(|(pub_key, value)| (pub_key.clone(), value.1)),
            vec![],
            Some(10),
        );
        // our active validator will be the first in the map
        let (pub_key, (keypair, _)) = validators.iter().next().unwrap();
        // this is necessary for the round exponent to be tracked - it only happens in the
        // ActiveValidator
        let _ =
            highway.activate_validator(pub_key.clone(), keypair.clone(), Timestamp::zero(), None);
        Self {
            highway,
            leaders: LeaderSequence::new(
                0, // used as the seed in `new_test_highway_protocol`
                &validators
                    .values()
                    .map(|(_keypair, weight)| (*weight).into())
                    .collect(),
                vec![true; validators.len()].into(),
            ),
            validators,
            current_round_start: 0,
            slow_validators,
            rng: NodeRng::new(),
            finalized_blocks: vec![],
        }
    }

    /// Gets the node 0 Highway instance as a reference to the `HighwayProtocol` struct.
    fn highway(&self) -> &HighwayProtocol<ClContext> {
        self.highway.as_any().downcast_ref().unwrap()
    }

    /// Gets the public key of node 0.
    fn our_pub_key(&self) -> &PublicKey {
        self.validators.keys().next().unwrap()
    }

    /// Returns `true` if node 0 is a slow node.
    fn is_slow(&self) -> bool {
        self.slow_validators.contains(self.our_pub_key())
    }

    /// Returns the minimum round length setting from the test chainspec generated for the
    /// simulation.
    fn min_round_len(&self) -> u64 {
        self.highway()
            .highway()
            .state()
            .params()
            .min_round_length()
            .millis()
    }

    /// Clones the protocol state of the Highway instance of node 0.
    /// This is used to save the state at some particular point, so that it can be used later for
    /// creation of units sent by other nodes, while the simulated internal state of node 0
    /// progresses.
    fn clone_state(&self) -> State<ClContext> {
        self.highway().highway().state().clone()
    }

    /// Simulates a round of consensus.
    /// In each round, the leader creates a proposal and a witness unit.
    /// If the leader is a slow node, other nodes only receive the proposal at the end of the round
    /// and create witness units without a proposal.
    /// Otherwise, the fast nodes will create a confirmation unit and a witness unit. The slow
    /// nodes will not receive the proposal and fast nodes' confirmation units before the witness
    /// timeout, so they will create witness units without citing a proposal also in this case.
    /// This makes it so that slow nodes effectively don't participate in the finalization of
    /// proposals, making their participation metrics very low.
    pub(super) fn crank_round(&mut self) {
        let min_round_len = self.min_round_len();
        let round_id = Timestamp::from(self.current_round_start);
        let leader = self.leaders.leader(round_id.millis());
        let leader_pub_key = self
            .validators
            .keys()
            .nth(leader.0 as usize)
            .unwrap()
            .clone();
        let leader_is_slow = self.slow_validators.contains(&leader_pub_key);

        let pre_proposal_state = self.clone_state();

        let (mut post_proposal_state, maybe_proposal_msg) = if leader.0 == 0 {
            // our active validator is the proposer
            self.this_node_propose();
            (self.clone_state(), None)
        } else {
            // another validator is the proposer
            self.other_node_propose(leader)
        };

        // if we're slow, we're going to create a witness unit before receiving any units from
        // other nodes, effectively not citing any of them
        // if we're the leader, our proposal and confirmation are already in the state at this
        // point
        if self.is_slow() {
            let timestamp = (self.current_round_start + min_round_len * 2 / 3).into();
            let outcomes = self.highway.handle_timer(
                timestamp,
                timestamp,
                TIMER_ID_ACTIVE_VALIDATOR,
                &mut self.rng,
            );
            self.finalize_blocks(&outcomes);
        }

        if leader_is_slow {
            // all validators will just send witness units, as they won't receive a proposal before
            // witness timeout
            let mut witness_units: Vec<_> = self
                .validators
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(vid, _)| *vid != leader.0 as usize)
                .map(|(vid, (_, (keypair, _)))| {
                    self.create_swunit_from(
                        (vid as u32).into(),
                        keypair,
                        &pre_proposal_state,
                        min_round_len * 2 / 3,
                        None,
                    )
                })
                .collect();
            if leader.0 != 0 {
                // the leader has to create a witness from the post-proposal state, or it will
                // equivocate
                let keypair = &self.validators.values().nth(leader.0 as usize).unwrap().0;
                witness_units.push(self.create_swunit_from(
                    leader.0.into(),
                    keypair,
                    &post_proposal_state,
                    min_round_len * 2 / 3,
                    None,
                ));
            }
            // we create a witness, too
            if !self.is_slow() {
                let timestamp = (self.current_round_start + min_round_len * 2 / 3).into();
                let outcomes = self.highway.handle_timer(
                    timestamp,
                    timestamp,
                    TIMER_ID_ACTIVE_VALIDATOR,
                    &mut self.rng,
                );
                self.finalize_blocks(&outcomes);
            }
            if let Some(proposal_msg) = maybe_proposal_msg {
                self.handle_message(proposal_msg, min_round_len * 3 / 4);
            }
            for unit in witness_units {
                let highway_msg = HighwayMessage::NewVertex(Vertex::Unit(unit));
                let msg = SerializedMessage::from_message(&highway_msg);
                self.handle_message(msg, min_round_len * 3 / 4);
            }

            self.add_vertices((self.current_round_start + min_round_len * 3 / 4 + 1).into());
        } else {
            // every fast validator creates a confirmation
            let fast_confirmation_units: Vec<_> = self
                .validators
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(vid, (pub_key, _))| {
                    !self.slow_validators.contains(pub_key) && *vid != leader.0 as usize
                })
                .map(|(vid, (_, (keypair, _)))| {
                    self.create_swunit_from(
                        (vid as u32).into(),
                        keypair,
                        &post_proposal_state,
                        min_round_len / 3,
                        None,
                    )
                })
                .collect();

            // add proposal and confirmations to our state
            if let Some(proposal_msg) = maybe_proposal_msg {
                self.handle_message(proposal_msg, min_round_len / 4);
            }
            for unit in fast_confirmation_units.clone() {
                let highway_msg = HighwayMessage::NewVertex(Vertex::Unit(unit));
                let msg = SerializedMessage::from_message(&highway_msg);
                self.handle_message(msg, min_round_len / 3 + 1);
            }
            self.add_vertices((self.current_round_start + min_round_len / 3 + 2).into());

            let post_confirmation_state = if self.is_slow() {
                // if we're slow, the post confirmation state should not contain our own
                // confirmation
                for unit in fast_confirmation_units {
                    post_proposal_state.add_valid_unit(unit);
                }
                post_proposal_state
            } else {
                self.clone_state()
            };

            // we create a witness at this point, if we aren't slow
            if !self.is_slow() {
                let timestamp = (self.current_round_start + min_round_len * 2 / 3).into();
                self.highway.handle_timer(
                    timestamp,
                    timestamp,
                    TIMER_ID_ACTIVE_VALIDATOR,
                    &mut self.rng,
                );
            }

            let fast_witness_units: Vec<_> = self
                .validators
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, (pub_key, _))| !self.slow_validators.contains(pub_key))
                .map(|(vid, (_, (keypair, _)))| {
                    self.create_swunit_from(
                        (vid as u32).into(),
                        keypair,
                        &post_confirmation_state,
                        min_round_len * 2 / 3,
                        None,
                    )
                })
                .collect();
            for unit in fast_witness_units {
                let highway_msg = HighwayMessage::NewVertex(Vertex::Unit(unit));
                let msg = SerializedMessage::from_message(&highway_msg);
                self.handle_message(msg, min_round_len * 3 / 4);
            }
            self.add_vertices((self.current_round_start + min_round_len * 3 / 4 + 1).into());

            // Slow nodes create witnesses before they can receive the proposal
            let slow_witness_units: Vec<_> = self
                .validators
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, (pub_key, _))| self.slow_validators.contains(pub_key))
                .map(|(vid, (_, (keypair, _)))| {
                    self.create_swunit_from(
                        (vid as u32).into(),
                        keypair,
                        &pre_proposal_state,
                        min_round_len * 2 / 3,
                        None,
                    )
                })
                .collect();
            for unit in slow_witness_units {
                let highway_msg = HighwayMessage::NewVertex(Vertex::Unit(unit));
                let msg = SerializedMessage::from_message(&highway_msg);
                self.handle_message(msg, min_round_len * 3 / 4);
            }
            self.add_vertices((self.current_round_start + min_round_len * 3 / 4 + 1).into());
        };

        self.current_round_start = self.current_round_start.saturating_add(min_round_len);
    }

    /// Simulates a proposal being sent by node 0. This is done by triggering the active validator
    /// timer at the timestamp when node 0 should make a proposal, and then calling
    /// `self.highway.propose()` with the context returned by the timer handler.
    fn this_node_propose(&mut self) {
        if (self.current_round_start / self.min_round_len()).trailing_zeros()
            < self.our_round_exp() as u32
        {
            // we're not proposing if we shouldn't even be participating in this round
            return;
        }

        let now: Timestamp = self.current_round_start.into();
        // the timer triggers a request for block content
        let outcomes =
            self.highway
                .handle_timer(now, now, TIMER_ID_ACTIVE_VALIDATOR, &mut self.rng);
        self.finalize_blocks(&outcomes);
        // the request contains necessary block context - extract it
        let block_context = outcomes
            .iter()
            .find_map(|outcome| match outcome {
                ProtocolOutcome::CreateNewBlock(context) => Some(context.clone()),
                _ => None,
            })
            .unwrap_or_else(|| {
                self.dump();
                panic!("outcomes didn't contain CreateNewBlock: {:?}", outcomes)
            });
        // this should create the proposal unit, add it to the state and create a message to be
        // broadcast - we can ignore the message, because we don't keep other consensus
        // instances
        let outcomes = self
            .highway
            .propose(ProposedBlock::new(Default::default(), block_context), now);
        self.finalize_blocks(&outcomes);
    }

    /// Creates a simulated `SignedWireUnit` sent by `creator`. This is used to simulate other
    /// nodes participating in consensus. The created unit uses `state.panorama()` as the cited
    /// panorama, which enables simulating the creation of units at a particular point in time
    /// (by passing a clone of the state from that point in time).
    fn create_swunit_from(
        &self,
        creator: ValidatorIndex,
        keypair: &Keypair,
        state: &State<ClContext>,
        delay: u64,
        value: Option<<ClContext as Context>::ConsensusValue>,
    ) -> SignedWireUnit<ClContext> {
        let seq_number = {
            let prev_unit_hash = state.panorama().get(creator).unwrap().correct();
            prev_unit_hash.map_or(0, |hash| state.unit(hash).seq_number.saturating_add(1))
        };
        let wunit: WireUnit<ClContext> = WireUnit {
            panorama: state.panorama().clone(),
            creator,
            instance_id: *self.highway().instance_id(),
            value,
            seq_number,
            timestamp: (self.current_round_start + delay).into(),
            round_exp: 0,
            endorsed: BTreeSet::new(),
        };
        SignedWireUnit::new(wunit.into_hashed(), keypair)
    }

    /// Simulates a proposal being sent by a node other than node 0. This is just a message
    /// containing a `SignedWireUnit` with a default consensus value, with the timestamp at the
    /// start of the current round.
    fn other_node_propose(
        &mut self,
        leader: ValidatorIndex,
    ) -> (State<ClContext>, Option<SerializedMessage>) {
        let (_pub_key, (keypair, _weight)) = self.validators.iter().nth(leader.0 as usize).unwrap();
        let state = self.highway().highway().state();
        let swunit = self.create_swunit_from(leader, keypair, state, 0, Some(Default::default()));

        let mut state_clone = state.clone();
        state_clone.add_valid_unit(swunit.clone());

        let highway_message: HighwayMessage<ClContext> =
            HighwayMessage::NewVertex(Vertex::Unit(swunit));
        let msg = SerializedMessage::from_message(&highway_message);

        (state_clone, Some(msg))
    }

    /// Handles a consensus message received from another node.
    /// If the message is a vertex (which is the only simulated case in this environment), it will
    /// become queued in the synchronizer, and will only be added to the protocol state once
    /// `add_vertices` is called.
    fn handle_message(&mut self, msg: SerializedMessage, delay: u64) {
        let outcomes = self.highway.handle_message(
            &mut self.rng,
            *ALICE_NODE_ID,
            msg,
            (self.current_round_start + delay).into(),
        );
        self.finalize_blocks(&outcomes);
    }

    /// Processes the vertices queued in the synchronizer of the Highway instance and adds them to
    /// the protocol state.
    fn add_vertices(&mut self, timestamp: Timestamp) {
        loop {
            let outcomes = self.highway.handle_action(ACTION_ID_VERTEX, timestamp);
            self.finalize_blocks(&outcomes);
            if !outcomes
                .iter()
                .any(|outcome| matches!(outcome, ProtocolOutcome::QueueAction(_)))
            {
                break;
            }
        }
    }

    /// Saves the finalized blocks that were output by the Highway instance.
    fn finalize_blocks(&mut self, outcomes: &[ProtocolOutcome<ClContext>]) {
        for outcome in outcomes {
            if let ProtocolOutcome::FinalizedBlock(block) = outcome {
                self.finalized_blocks.push(block.clone());
            }
        }
    }

    /// Gets the current round exponent set in the Highway instance. It is being changed by the
    /// instance itself internally in response to how the protocol state progresses.
    pub(super) fn our_round_exp(&self) -> u8 {
        self.highway().highway().get_round_exp().unwrap()
    }

    /// For test debugging purposes
    #[allow(unused)]
    pub(super) fn dump(&self) {
        let dump = EraDump {
            id: 0.into(),
            start_time: 0.into(),
            accusations: &Default::default(),
            cannot_propose: &Default::default(),
            faulty: &Default::default(),
            start_height: 0,
            validators: &self
                .validators
                .iter()
                .map(|(pub_key, (_, weight))| (pub_key.clone(), (*weight).into()))
                .collect(),
            highway_state: self.highway().highway().state(),
        };

        let mut file = File::create("/tmp/consensus.dump").unwrap();
        let data = bincode::serialize(&dump).unwrap();
        let _ = file.write_all(&data);
    }
}
