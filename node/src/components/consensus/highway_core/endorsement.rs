use crate::components::consensus::traits::Context;

pub(crate) struct Endorsement<C: Context> {
    message_hash: C::Hash,
    endorser: C::ValidatorId,
}
