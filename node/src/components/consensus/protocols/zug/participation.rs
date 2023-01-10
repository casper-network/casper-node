use std::fmt::Debug;

use crate::components::consensus::{
    protocols::zug::{Fault, RoundId, Zug},
    traits::Context,
    utils::ValidatorIndex,
};

/// A map of status (faulty, inactive) by validator ID.
#[derive(Debug)]
// False positive, as the fields of this struct are all used in logging validator participation.
#[allow(dead_code)]
pub(super) struct Participation<C>
where
    C: Context,
{
    pub(super) instance_id: C::InstanceId,
    pub(super) faulty_stake_percent: u8,
    pub(super) inactive_stake_percent: u8,
    pub(super) inactive_validators: Vec<(ValidatorIndex, C::ValidatorId, ParticipationStatus)>,
    pub(super) faulty_validators: Vec<(ValidatorIndex, C::ValidatorId, ParticipationStatus)>,
}

/// A validator's participation status: whether they are faulty or inactive.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub(super) enum ParticipationStatus {
    LastSeenInRound(RoundId),
    Inactive,
    EquivocatedInOtherEra,
    Equivocated,
}

impl ParticipationStatus {
    /// Returns a `Status` for a validator unless they are honest and online.
    pub(super) fn for_index<C: Context + 'static>(
        idx: ValidatorIndex,
        zug: &Zug<C>,
    ) -> Option<ParticipationStatus> {
        if let Some(fault) = zug.faults.get(&idx) {
            return Some(match fault {
                Fault::Banned | Fault::Indirect => ParticipationStatus::EquivocatedInOtherEra,
                Fault::Direct(..) => ParticipationStatus::Equivocated,
            });
        }
        // TODO: Avoid iterating over all old rounds every time we log this.
        for r_id in zug.rounds.keys().rev() {
            if zug.has_echoed(*r_id, idx)
                || zug.has_voted(*r_id, idx)
                || (zug.has_accepted_proposal(*r_id) && zug.leader(*r_id) == idx)
            {
                if r_id.saturating_add(2) < zug.current_round {
                    return Some(ParticipationStatus::LastSeenInRound(*r_id));
                } else {
                    return None; // Seen recently; considered currently active.
                }
            }
        }
        Some(ParticipationStatus::Inactive)
    }
}
