use datasize::DataSize;

use crate::types::FinalitySignature;

#[derive(DataSize, Debug)]
pub(super) enum Signature {
    Local(Box<FinalitySignature>),
    External(Box<FinalitySignature>),
}

impl Signature {
    pub(super) fn to_inner(&self) -> &FinalitySignature {
        match self {
            Signature::Local(fs) => fs,
            Signature::External(fs) => fs,
        }
    }

    pub(super) fn take(self) -> Box<FinalitySignature> {
        match self {
            Signature::Local(fs) | Signature::External(fs) => fs,
        }
    }

    pub(super) fn is_local(&self) -> bool {
        matches!(self, Signature::Local(_))
    }
}
