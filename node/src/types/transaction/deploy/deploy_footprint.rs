use casper_types::{bytesrepr::ToBytes, Deploy, DeployError, DeployHeader, Gas};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Information about how much block limit a [`Deploy`] will consume.
#[derive(Clone, Debug, DataSize, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeployFootprint {
    /// The header of the `Deploy`.
    pub(crate) header: DeployHeader,
    /// The estimated gas consumption of the `Deploy`.
    pub(crate) gas_estimate: Gas,
    /// The bytesrepr serialized length of the `Deploy`.
    pub(crate) size_estimate: usize,
    /// Whether the `Deploy` is a transfer or not.
    pub(crate) is_transfer: bool,
}

pub(crate) trait DeployExt {
    fn footprint(&self) -> Result<DeployFootprint, DeployError>;
}

impl DeployExt for Deploy {
    /// Returns the `DeployFootprint`.
    fn footprint(&self) -> Result<DeployFootprint, DeployError> {
        let header = self.header().clone();
        let gas_estimate = match self.payment().payment_amount(header.gas_price()) {
            Some(gas) => gas,
            None => {
                return Err(DeployError::InvalidPayment);
            }
        };
        let size_estimate = self.serialized_length();
        let is_transfer = self.session().is_transfer();
        Ok(DeployFootprint {
            header,
            gas_estimate,
            size_estimate,
            is_transfer,
        })
    }
}
