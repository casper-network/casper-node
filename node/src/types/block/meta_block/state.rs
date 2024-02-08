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
    pub(super) stored: bool,
    pub(super) sent_to_deploy_buffer: bool,
    pub(super) updated_validator_matrix: bool,
    pub(super) gossiped: bool,
    pub(super) executed: bool,
    pub(super) tried_to_sign: bool,
    pub(super) consensus_notified: bool,
    pub(super) accumulator_notified: bool,
    pub(super) synchronizer_notified: bool,
    pub(super) validator_notified: bool,
    pub(super) sufficient_finality: bool,
    pub(super) marked_complete: bool,
    pub(super) all_actions_done: bool,
}

impl State {
    /// Returns a new `State` with all fields set to `false`.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns a new `State` with all fields set to `false` except for `gossiped`.
    pub(crate) fn new_not_to_be_gossiped() -> Self {
        State {
            gossiped: true,
            ..Self::default()
        }
    }

    /// Returns a new `State` with all fields set to `false` except for `stored`.
    pub(crate) fn new_already_stored() -> Self {
        State {
            stored: true,
            ..Self::default()
        }
    }

    /// Returns a new `State` which a historical block is expected to have after it has been synced.
    pub(crate) fn new_after_historical_sync() -> Self {
        State {
            stored: true,
            sent_to_deploy_buffer: false,
            updated_validator_matrix: true,
            gossiped: true,
            executed: true,
            tried_to_sign: true,
            consensus_notified: false,
            accumulator_notified: true,
            synchronizer_notified: true,
            validator_notified: false,
            sufficient_finality: true,
            marked_complete: true,
            all_actions_done: false,
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

    pub(crate) fn register_as_consensus_notified(&mut self) -> StateChange {
        let outcome = StateChange::from(self.consensus_notified);
        self.consensus_notified = true;
        outcome
    }

    pub(crate) fn register_as_accumulator_notified(&mut self) -> StateChange {
        let outcome = StateChange::from(self.accumulator_notified);
        self.accumulator_notified = true;
        outcome
    }

    pub(crate) fn register_as_synchronizer_notified(&mut self) -> StateChange {
        let outcome = StateChange::from(self.synchronizer_notified);
        self.synchronizer_notified = true;
        outcome
    }

    pub(crate) fn register_as_validator_notified(&mut self) -> StateChange {
        let outcome = StateChange::from(self.validator_notified);
        self.validator_notified = true;
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

    pub(crate) fn register_all_actions_done(&mut self) -> StateChange {
        let outcome = StateChange::from(self.all_actions_done);
        self.all_actions_done = true;
        outcome
    }

    pub(super) fn merge(mut self, other: State) -> Result<Self, MergeMismatchError> {
        let State {
            ref mut stored,
            ref mut sent_to_deploy_buffer,
            ref mut updated_validator_matrix,
            ref mut gossiped,
            ref mut executed,
            ref mut tried_to_sign,
            ref mut consensus_notified,
            ref mut accumulator_notified,
            ref mut synchronizer_notified,
            ref mut validator_notified,
            ref mut sufficient_finality,
            ref mut marked_complete,
            ref mut all_actions_done,
        } = self;

        *stored |= other.stored;
        *sent_to_deploy_buffer |= other.sent_to_deploy_buffer;
        *updated_validator_matrix |= other.updated_validator_matrix;
        *gossiped |= other.gossiped;
        *executed |= other.executed;
        *tried_to_sign |= other.tried_to_sign;
        *consensus_notified |= other.consensus_notified;
        *accumulator_notified |= other.accumulator_notified;
        *synchronizer_notified |= other.synchronizer_notified;
        *validator_notified |= other.validator_notified;
        *sufficient_finality |= other.sufficient_finality;
        *marked_complete |= other.marked_complete;
        *all_actions_done |= other.all_actions_done;

        Ok(self)
    }

    pub(crate) fn verify_complete(&self) -> bool {
        self.stored
            && self.sent_to_deploy_buffer
            && self.updated_validator_matrix
            && self.gossiped
            && self.executed
            && self.tried_to_sign
            && self.consensus_notified
            && self.accumulator_notified
            && self.synchronizer_notified
            && self.validator_notified
            && self.sufficient_finality
            && self.marked_complete
    }

    #[cfg(test)]
    pub(crate) fn set_sufficient_finality(&mut self, has_sufficient_finality: bool) {
        self.sufficient_finality = has_sufficient_finality;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_merge() {
        let all_true = State {
            stored: true,
            sent_to_deploy_buffer: true,
            updated_validator_matrix: true,
            gossiped: true,
            executed: true,
            tried_to_sign: true,
            consensus_notified: true,
            accumulator_notified: true,
            synchronizer_notified: true,
            validator_notified: true,
            sufficient_finality: true,
            marked_complete: true,
            all_actions_done: true,
        };
        let all_false = State::default();

        assert_eq!(all_true.merge(all_false).unwrap(), all_true);
        assert_eq!(all_false.merge(all_true).unwrap(), all_true);
        assert_eq!(all_true.merge(all_true).unwrap(), all_true);
        assert_eq!(all_false.merge(all_false).unwrap(), all_false);
    }
}
