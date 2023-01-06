use datasize::DataSize;
use serde::Serialize;

use super::MergeMismatchError;

#[derive(Clone, Copy, Eq, PartialEq, Debug, DataSize)]
pub(crate) enum StateChange {
    Updated,
    AlreadySet,
}

impl From<bool> for StateChange {
    fn from(current_state: bool) -> Self {
        if current_state {
            StateChange::AlreadySet
        } else {
            StateChange::Updated
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Default, Serialize, Debug, DataSize)]
pub(crate) struct State {
    pub(super) is_immediate_switch_block_for_current_protocol_version: bool,
    pub(super) is_stored: bool,
    pub(super) has_been_sent_to_deploy_buffer: bool,
    pub(super) has_updated_validator_matrix: bool,
    pub(super) has_been_gossiped: bool,
    pub(super) is_executed: bool,
    pub(super) we_have_tried_to_sign: bool,
    pub(super) has_been_sent_to_consensus_post_execution: bool,
    pub(super) has_been_sent_to_accumulator_post_execution: bool,
    pub(super) has_sufficient_finality: bool,
    pub(super) is_marked_complete: bool,
}

impl State {
    /// Returns a new `State` with all fields set to `false`.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns a new `State` with all fields set to `false` except for
    /// `is_immediate_switch_block_for_current_protocol_version`.
    pub(crate) fn new_immediate_switch() -> Self {
        State {
            is_immediate_switch_block_for_current_protocol_version: true,
            ..Self::default()
        }
    }

    /// Returns a new `State` with all fields set to `false` except for `is_stored`.
    pub(crate) fn new_synced() -> Self {
        State {
            is_stored: true,
            ..Self::default()
        }
    }

    pub(crate) fn is_stored(&self) -> bool {
        self.is_stored
    }

    pub(crate) fn is_executed(&self) -> bool {
        self.is_executed
    }

    pub(crate) fn has_sufficient_finality(&self) -> bool {
        self.has_sufficient_finality
    }

    pub(crate) fn is_marked_complete(&self) -> bool {
        self.is_marked_complete
    }

    pub(crate) fn register_as_stored(&mut self) -> StateChange {
        let outcome = StateChange::from(self.is_stored);
        self.is_stored = true;
        outcome
    }

    pub(crate) fn register_as_sent_to_deploy_buffer(&mut self) -> StateChange {
        let outcome = StateChange::from(self.has_been_sent_to_deploy_buffer);
        self.has_been_sent_to_deploy_buffer = true;
        outcome
    }

    pub(crate) fn register_updated_validator_matrix(&mut self) -> StateChange {
        let outcome = StateChange::from(self.has_updated_validator_matrix);
        self.has_updated_validator_matrix = true;
        outcome
    }

    pub(crate) fn register_as_gossiped(&mut self) -> StateChange {
        // We don't gossip immediate switch blocks
        if self.is_immediate_switch_block_for_current_protocol_version {
            return StateChange::AlreadySet;
        }
        let outcome = StateChange::from(self.has_been_gossiped);
        self.has_been_gossiped = true;
        outcome
    }

    pub(crate) fn register_as_executed(&mut self) -> StateChange {
        let outcome = StateChange::from(self.is_executed);
        self.is_executed = true;
        outcome
    }

    pub(crate) fn register_we_have_tried_to_sign(&mut self) -> StateChange {
        let outcome = StateChange::from(self.we_have_tried_to_sign);
        self.we_have_tried_to_sign = true;
        outcome
    }

    pub(crate) fn register_as_sent_to_consensus_post_execution(&mut self) -> StateChange {
        let outcome = StateChange::from(self.has_been_sent_to_consensus_post_execution);
        self.has_been_sent_to_consensus_post_execution = true;
        outcome
    }

    pub(crate) fn register_as_sent_to_accumulator_post_execution(&mut self) -> StateChange {
        let outcome = StateChange::from(self.has_been_sent_to_accumulator_post_execution);
        self.has_been_sent_to_accumulator_post_execution = true;
        outcome
    }

    pub(crate) fn register_has_sufficient_finality(&mut self) -> StateChange {
        let outcome = StateChange::from(self.has_sufficient_finality);
        self.has_sufficient_finality = true;
        outcome
    }

    pub(crate) fn register_as_marked_complete(&mut self) -> StateChange {
        let outcome = StateChange::from(self.is_marked_complete);
        self.is_marked_complete = true;
        outcome
    }

    pub(super) fn merge(mut self, other: State) -> Result<Self, MergeMismatchError> {
        let State {
            is_immediate_switch_block_for_current_protocol_version,
            ref mut is_stored,
            ref mut has_been_sent_to_deploy_buffer,
            ref mut has_updated_validator_matrix,
            ref mut has_been_gossiped,
            ref mut is_executed,
            ref mut we_have_tried_to_sign,
            ref mut has_been_sent_to_consensus_post_execution,
            ref mut has_been_sent_to_accumulator_post_execution,
            ref mut has_sufficient_finality,
            ref mut is_marked_complete,
        } = self;

        if is_immediate_switch_block_for_current_protocol_version
            != other.is_immediate_switch_block_for_current_protocol_version
        {
            return Err(MergeMismatchError::State);
        }

        *is_stored |= other.is_stored;
        *has_been_sent_to_deploy_buffer |= other.has_been_sent_to_deploy_buffer;
        *has_updated_validator_matrix |= other.has_updated_validator_matrix;
        *has_been_gossiped |= other.has_been_gossiped;
        *is_executed |= other.is_executed;
        *we_have_tried_to_sign |= other.we_have_tried_to_sign;
        *has_been_sent_to_consensus_post_execution |=
            other.has_been_sent_to_consensus_post_execution;
        *has_been_sent_to_accumulator_post_execution |=
            other.has_been_sent_to_accumulator_post_execution;
        *has_sufficient_finality |= other.has_sufficient_finality;
        *is_marked_complete |= other.is_marked_complete;

        Ok(self)
    }

    #[cfg(debug_assertions)]
    pub(crate) fn verify_complete(&self) -> bool {
        self.is_stored
            && self.has_been_sent_to_deploy_buffer
            && self.has_updated_validator_matrix
            && (self.has_been_gossiped
                || self.is_immediate_switch_block_for_current_protocol_version)
            && self.is_executed
            && self.we_have_tried_to_sign
            && self.has_been_sent_to_consensus_post_execution
            && self.has_been_sent_to_accumulator_post_execution
            && self.has_sufficient_finality
            && self.is_marked_complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_merge() {
        let all_true = State {
            is_immediate_switch_block_for_current_protocol_version: true,
            is_stored: true,
            has_been_sent_to_deploy_buffer: true,
            has_updated_validator_matrix: true,
            has_been_gossiped: true,
            is_executed: true,
            we_have_tried_to_sign: true,
            has_been_sent_to_consensus_post_execution: true,
            has_been_sent_to_accumulator_post_execution: true,
            has_sufficient_finality: true,
            is_marked_complete: true,
        };
        let all_false = State {
            // this must be set the same as the `all_true`'s - all other fields are `false`.
            is_immediate_switch_block_for_current_protocol_version: true,
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
            is_immediate_switch_block_for_current_protocol_version: true,
            ..State::default()
        };
        let state2 = State {
            is_immediate_switch_block_for_current_protocol_version: false,
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
