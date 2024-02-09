#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{Gas, TransactionV1Header};

use super::transaction_v1_category::TransactionV1Category;

/// Information about how much block limit a [`Transaction V1`] will consume.
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
    /// The estimated gas consumption of the `Deploy`.
    pub gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Transaction`.
    pub size_estimate: usize,
    /// Transaction session kind.
    pub category: TransactionV1Category,
}
