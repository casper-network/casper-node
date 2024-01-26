#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Deploy;
use super::DeployHeader;
use crate::Gas;

/// Information about how much block limit a [`Deploy`] will consume.
#[derive(Clone, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct DeployFootprint {
    /// The header of the `Deploy`.
    pub header: DeployHeader,
    /// The estimated gas consumption of the `Deploy`.
    pub gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Deploy`.
    pub size_estimate: usize,
    /// Whether the `Deploy` is a transfer or not.
    pub is_transfer: bool,
}
