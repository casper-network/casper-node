use std::cmp::Reverse;

use crate::{
    components::consensus::{
        highway_core::{
            highway::Highway,
            state::{Fault, State},
            validators::ValidatorIndex,
        },
        traits::Context,
    },
    types::Timestamp,
    utils::div_round,
};

/// A validator's participation status: whether they are faulty or inactive.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
enum Status {
    LastSeenSecondsAgo(u64),
    Inactive,
    EquivocatedInOtherEra,
    Equivocated,
}

impl Status {
    /// Returns a `Status` for a validator unless they are honest and online.
    fn for_index<C: Context>(
        idx: ValidatorIndex,
        state: &State<C>,
        now: Timestamp,
    ) -> Option<Status> {
        if let Some(fault) = state.maybe_fault(idx) {
            return Some(match fault {
                Fault::Banned | Fault::Indirect => Status::EquivocatedInOtherEra,
                Fault::Direct(_) => Status::Equivocated,
            });
        }
        if state.panorama()[idx].is_none() {
            return Some(Status::Inactive);
        }
        if state.last_seen(idx) + state.params().max_round_length() < now {
            let seconds = now.saturating_diff(state.last_seen(idx)).millis() / 1000;
            return Some(Status::LastSeenSecondsAgo(seconds));
        }
        None
    }
}

/// A map of status (faulty, inactive) by validator ID.
#[derive(Debug)]
// False positive, as the fields of this struct are all used in logging validator participation.
#[allow(dead_code)]
pub(crate) struct Participation<C>
where
    C: Context,
{
    instance_id: C::InstanceId,
    faulty_stake_percent: u8,
    inactive_stake_percent: u8,
    inactive_validators: Vec<(ValidatorIndex, C::ValidatorId, Status)>,
    faulty_validators: Vec<(ValidatorIndex, C::ValidatorId, Status)>,
}

impl<C: Context> Participation<C> {
    /// Creates a new `Participation` map, showing validators seen as faulty or inactive by the
    /// Highway instance.
    #[allow(clippy::integer_arithmetic)] // We use u128 to prevent overflows in weight calculation.
    pub(crate) fn new(highway: &Highway<C>) -> Self {
        let now = Timestamp::now();
        let state = highway.state();
        let mut inactive_w = 0;
        let mut faulty_w = 0;
        let total_w = u128::from(state.total_weight().0);
        let mut inactive_validators = Vec::new();
        let mut faulty_validators = Vec::new();
        for (idx, v_id) in highway.validators().enumerate_ids() {
            if let Some(status) = Status::for_index(idx, state, now) {
                match status {
                    Status::Equivocated | Status::EquivocatedInOtherEra => {
                        faulty_w += u128::from(state.weight(idx).0);
                        faulty_validators.push((idx, v_id.clone(), status));
                    }
                    Status::Inactive | Status::LastSeenSecondsAgo(_) => {
                        inactive_w += u128::from(state.weight(idx).0);
                        inactive_validators.push((idx, v_id.clone(), status));
                    }
                }
            }
        }
        inactive_validators.sort_by_key(|(idx, _, status)| (Reverse(*status), *idx));
        faulty_validators.sort_by_key(|(idx, _, status)| (Reverse(*status), *idx));
        Participation {
            instance_id: *highway.instance_id(),
            inactive_stake_percent: div_round(inactive_w * 100, total_w) as u8,
            faulty_stake_percent: div_round(faulty_w * 100, total_w) as u8,
            inactive_validators,
            faulty_validators,
        }
    }
}
