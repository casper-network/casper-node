use std::cmp::Ordering;

use casper_types::EraId;
use datasize::DataSize;

#[derive(Clone, Copy, DataSize, Debug, Eq, PartialEq)]
pub(super) struct LocalTipIdentifier {
    pub(super) height: u64,
    pub(super) era_id: EraId,
}

impl LocalTipIdentifier {
    pub(super) fn new(height: u64, era_id: EraId) -> Self {
        Self { height, era_id }
    }
}

impl PartialOrd for LocalTipIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.height.partial_cmp(&other.height)
    }
}

impl Ord for LocalTipIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.height.cmp(&other.height)
    }
}
