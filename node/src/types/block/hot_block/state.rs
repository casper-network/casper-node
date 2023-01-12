use datasize::DataSize;
use serde::Serialize;

use super::MergeMismatchError;

#[derive(Clone, Copy, Debug, DataSize)]
pub(crate) enum StateChange {
    Updated,
    AlreadyRegistered,
}

impl StateChange {
    pub(crate) fn was_updated(self) -> bool {
        matches!(self, StateChange::Updated)
    }

    pub(crate) fn was_already_registered(self) -> bool {
        matches!(self, StateChange::AlreadyRegistered)
    }
}

impl From<bool> for StateChange {
    fn from(current_state: bool) -> Self {
        if current_state {
            StateChange::AlreadyRegistered
        } else {
            StateChange::Updated
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Default, Serialize, Debug, DataSize)]
pub(crate) struct State {
    pub(super) immediate_switch_block_for_current_protocol_version: bool,
    pub(super) stored: bool,
    pub(super) sent_to_deploy_buffer: bool,
    pub(super) updated_validator_matrix: bool,
    pub(super) gossiped: bool,
    pub(super) executed: bool,
    pub(super) tried_to_sign: bool,
    pub(super) sent_to_consensus_post_execution: bool,
    pub(super) sent_to_accumulator_post_execution: bool,
    pub(super) sufficient_finality: bool,
    pub(super) marked_complete: bool,
}

impl State {
    /// Returns a new `State` with all fields set to `false`.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns a new `State` with all fields set to `false` except for
    /// `immediate_switch_block_for_current_protocol_version`.
    pub(crate) fn new_immediate_switch() -> Self {
        State {
            immediate_switch_block_for_current_protocol_version: true,
            ..Self::default()
        }
    }

    /// Returns a new `State` with all fields set to `false` except for `stored`.
    pub(crate) fn new_synced() -> Self {
        State {
            stored: true,
            ..Self::default()
        }
    }

    /// Returns a new `State` with all fields set to `true` except for `sent_to_deploy_buffer` and
    /// `sent_to_consensus_post_execution`.
    pub(crate) fn new_after_historical_sync() -> Self {
        State {
            immediate_switch_block_for_current_protocol_version: false,
            stored: true,
            sent_to_deploy_buffer: false,
            updated_validator_matrix: true,
            gossiped: true,
            executed: true,
            tried_to_sign: true,
            sent_to_consensus_post_execution: false,
            sent_to_accumulator_post_execution: true,
            sufficient_finality: true,
            marked_complete: true,
        }
    }

    pub(crate) fn is_stored(&self) -> bool {
        self.stored
    }

    pub(crate) fn is_executed(&self) -> bool {
        self.executed
    }

    pub(crate) fn has_sufficient_finality(&self) -> bool {
        self.sufficient_finality
    }

    pub(crate) fn is_marked_complete(&self) -> bool {
        self.marked_complete
    }

    pub(crate) fn register_as_stored(&mut self) -> StateChange {
        let outcome = StateChange::from(self.stored);
        self.stored = true;
        outcome
    }

    pub(crate) fn register_as_sent_to_deploy_buffer(&mut self) -> StateChange {
        let outcome = StateChange::from(self.sent_to_deploy_buffer);
        self.sent_to_deploy_buffer = true;
        outcome
    }

    pub(crate) fn register_updated_validator_matrix(&mut self) -> StateChange {
        let outcome = StateChange::from(self.updated_validator_matrix);
        self.updated_validator_matrix = true;
        outcome
    }

    pub(crate) fn register_as_gossiped(&mut self) -> StateChange {
        // We don't gossip immediate switch blocks
        if self.immediate_switch_block_for_current_protocol_version {
            return StateChange::AlreadyRegistered;
        }
        let outcome = StateChange::from(self.gossiped);
        self.gossiped = true;
        outcome
    }

    pub(crate) fn register_as_executed(&mut self) -> StateChange {
        let outcome = StateChange::from(self.executed);
        self.executed = true;
        outcome
    }

    pub(crate) fn register_we_have_tried_to_sign(&mut self) -> StateChange {
        let outcome = StateChange::from(self.tried_to_sign);
        self.tried_to_sign = true;
        outcome
    }

    pub(crate) fn register_as_sent_to_consensus_post_execution(&mut self) -> StateChange {
        let outcome = StateChange::from(self.sent_to_consensus_post_execution);
        self.sent_to_consensus_post_execution = true;
        outcome
    }

    pub(crate) fn register_as_sent_to_accumulator_post_execution(&mut self) -> StateChange {
        let outcome = StateChange::from(self.sent_to_accumulator_post_execution);
        self.sent_to_accumulator_post_execution = true;
        outcome
    }

    pub(crate) fn register_has_sufficient_finality(&mut self) -> StateChange {
        let outcome = StateChange::from(self.sufficient_finality);
        self.sufficient_finality = true;
        outcome
    }

    pub(crate) fn register_as_marked_complete(&mut self) -> StateChange {
        let outcome = StateChange::from(self.marked_complete);
        self.marked_complete = true;
        outcome
    }

    pub(super) fn merge(mut self, other: State) -> Result<Self, MergeMismatchError> {
        let State {
            immediate_switch_block_for_current_protocol_version,
            ref mut stored,
            ref mut sent_to_deploy_buffer,
            ref mut updated_validator_matrix,
            ref mut gossiped,
            ref mut executed,
            ref mut tried_to_sign,
            ref mut sent_to_consensus_post_execution,
            ref mut sent_to_accumulator_post_execution,
            ref mut sufficient_finality,
            ref mut marked_complete,
        } = self;

        if immediate_switch_block_for_current_protocol_version
            != other.immediate_switch_block_for_current_protocol_version
        {
            return Err(MergeMismatchError::State);
        }

        *stored |= other.stored;
        *sent_to_deploy_buffer |= other.sent_to_deploy_buffer;
        *updated_validator_matrix |= other.updated_validator_matrix;
        *gossiped |= other.gossiped;
        *executed |= other.executed;
        *tried_to_sign |= other.tried_to_sign;
        *sent_to_consensus_post_execution |= other.sent_to_consensus_post_execution;
        *sent_to_accumulator_post_execution |= other.sent_to_accumulator_post_execution;
        *sufficient_finality |= other.sufficient_finality;
        *marked_complete |= other.marked_complete;

        Ok(self)
    }

    pub(crate) fn verify_complete(&self) -> bool {
        self.stored
            && self.sent_to_deploy_buffer
            && self.updated_validator_matrix
            && (self.gossiped || self.immediate_switch_block_for_current_protocol_version)
            && self.executed
            && self.tried_to_sign
            && self.sent_to_consensus_post_execution
            && self.sent_to_accumulator_post_execution
            && self.sufficient_finality
            && self.marked_complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_merge() {
        let all_true = State {
            immediate_switch_block_for_current_protocol_version: true,
            stored: true,
            sent_to_deploy_buffer: true,
            updated_validator_matrix: true,
            gossiped: true,
            executed: true,
            tried_to_sign: true,
            sent_to_consensus_post_execution: true,
            sent_to_accumulator_post_execution: true,
            sufficient_finality: true,
            marked_complete: true,
        };
        let all_false = State {
            // this must be set the same as the `all_true`'s - all other fields are `false`.
            immediate_switch_block_for_current_protocol_version: true,
            ..State::default()
        };

        assert_eq!(all_true.merge(all_false).unwrap(), all_true);
        assert_eq!(all_false.merge(all_true).unwrap(), all_true);
        assert_eq!(all_true.merge(all_true).unwrap(), all_true);
        assert_eq!(all_false.merge(all_false).unwrap(), all_false);
    }

    #[test]
    fn should_fail_to_merge_different_immediate_switch_block_states() {
        let state1 = State {
            immediate_switch_block_for_current_protocol_version: true,
            ..State::default()
        };
        let state2 = State {
            immediate_switch_block_for_current_protocol_version: false,
            ..State::default()
        };

        assert!(matches!(
            state1.merge(state2),
            Err(MergeMismatchError::State)
        ));
        assert!(matches!(
            state2.merge(state1),
            Err(MergeMismatchError::State)
        ));
    }
}
