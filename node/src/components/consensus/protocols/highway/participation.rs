use std::cmp::Reverse;

use itertools::Itertools;

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
            let seconds = (now - state.last_seen(idx)).millis() / 1000;
            return Some(Status::LastSeenSecondsAgo(seconds));
        }
        None
    }
}

/// A map of status (faulty, inactive) by validator ID.
#[derive(Debug)]
pub(crate) struct Participation<C>
where
    C: Context,
{
    instance_id: C::InstanceId,
    faulty_stake_percent: u8,
    inactive_stake_percent: u8,
    /// The faulty and inactive validators, by ID and index.
    validators: Vec<(ValidatorIndex, C::ValidatorId, Status)>,
}

impl<C: Context> Participation<C> {
    /// Creates a new `Participation` map, showing validators seen as faulty or inactive by the
    /// Highway instance.
    pub(crate) fn new(highway: &Highway<C>) -> Self {
        let now = Timestamp::now();
        let state = highway.state();
        let mut inactive_w = 0;
        let mut faulty_w = 0;
        let total_w = u128::from(state.total_weight().0);
        let validators = highway
            .validators()
            .enumerate_ids()
            .filter_map(|(idx, v_id)| {
                let status = Status::for_index(idx, state, now)?;
                match status {
                    Status::Equivocated | Status::EquivocatedInOtherEra => {
                        faulty_w += u128::from(state.weight(idx).0)
                    }
                    Status::Inactive | Status::LastSeenSecondsAgo(_) => {
                        inactive_w += u128::from(state.weight(idx).0)
                    }
                }
                Some((idx, v_id.clone(), status))
            })
            .sorted_by_key(|(idx, _, status)| (Reverse(*status), *idx))
            .collect();
        Participation {
            instance_id: *highway.instance_id(),
            inactive_stake_percent: div_round(inactive_w * 100, total_w) as u8,
            faulty_stake_percent: div_round(faulty_w * 100, total_w) as u8,
            validators,
        }
    }
}
