use datasize::DataSize;

use crate::components::consensus::{
    protocols::zug::{Content, SignedMessage},
    traits::Context,
};

/// A reason for a validator to be marked as faulty.
///
/// The `Banned` state is fixed from the beginning and can't be replaced. However, `Indirect` can
/// be replaced with `Direct` evidence, which has the same effect but doesn't rely on information
/// from other consensus protocol instances.
#[derive(DataSize, Debug, PartialEq)]
pub(crate) enum Fault<C>
where
    C: Context,
{
    /// The validator was known to be malicious from the beginning. All their messages are
    /// considered invalid in this `Zug` instance.
    Banned,
    /// We have direct evidence of the validator's fault: two conflicting signatures.
    Direct(SignedMessage<C>, Content<C>, C::Signature),
    /// The validator is known to be faulty, but the evidence is not in this era.
    Indirect,
}

impl<C: Context> Fault<C> {
    pub(super) fn is_direct(&self) -> bool {
        matches!(self, Fault::Direct(..))
    }
}
