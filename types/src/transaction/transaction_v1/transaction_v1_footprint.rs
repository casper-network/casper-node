use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::TransactionV1Header;

#[derive(Clone, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct TransactionV1Footprint {
    /// The header of the `Transaction`.
    pub header: TransactionV1Header,
    /// Whether the `Transaction` is a transfer or not.
    pub is_transfer: bool,
}

impl TransactionV1Footprint {
    /// Returns `true` if this transaction is a native transfer.
    pub fn is_transfer(&self) -> bool {
        self.is_transfer
    }
}
