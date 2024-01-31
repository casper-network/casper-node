#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{Gas, TransactionV1Header};

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
    /// The estimated gas consumption of the `Transaction`.
    pub gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Transaction`.
    pub size_estimate: usize,
    /// Whether the `Transaction` is a transfer or not.
    // TODO[RC]: Is this distinction still correct for V1?
    pub is_transfer: bool,
}

impl TransactionV1Footprint {
    /// Returns `true` if this transaction is a native transfer.
    pub fn is_transfer(&self) -> bool {
        self.is_transfer
    }
}
