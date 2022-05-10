use datasize::DataSize;

use crate::components::consensus::{protocols::simple_consensus::Message, traits::Context};

/// A reason for a validator to be marked as faulty.
///
/// The `Banned` state is fixed from the beginning and can't be replaced. However, `Indirect` can
/// be replaced with `Direct` evidence, which has the same effect but doesn't rely on information
/// from other consensus protocol instances.
#[derive(DataSize, Debug)]
pub(crate) enum Fault<C>
where
    C: Context,
{
    /// The validator was known to be malicious from the beginning. All their messages are
    /// considered invalid in this `SimpleConsensus` instance.
    Banned,
    /// We have direct evidence of the validator's fault.
    // TODO: Store only the necessary information, e.g. not the full signed proposal, and only one
    // round ID, instance ID and validator index.
    Direct(Message<C>, Message<C>),
    /// The validator is known to be faulty, but the evidence is not in this era.
    Indirect,
}

impl<C: Context> Fault<C> {
    pub(super) fn is_direct(&self) -> bool {
        matches!(self, Fault::Direct(_, _))
    }

    pub(super) fn is_banned(&self) -> bool {
        matches!(self, Fault::Banned)
    }
}
