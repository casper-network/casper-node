#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Motes, TimeDiff, U512,
};

#[cfg(any(feature = "testing", test))]
pub const MAX_PAYMENT_AMOUNT: u64 = 2_500_000_000;

/// Configuration values associated with deploys.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    /// Maximum amount any deploy can pay.
    pub max_payment_cost: Motes,
    /// Maximum time to live any deploy can specify.
    pub max_ttl: TimeDiff,
    /// Maximum time to live any deploy can specify.
    #[deprecated(
        since = "1.0.0",
        note = "vestigial field from pre-release; it has never been active in a production release (is ignored), and will be removed in 2.0.0"
    )]
    pub max_dependencies: u8,
    /// Maximum possible size in bytes of a block (one of several block limits).
    pub max_block_size: u32,
    /// Maximum possible size in bytes of a single deploy.
    pub max_deploy_size: u32,
    /// Maximum number of standard deploys allowed in a block (one of several block limits).
    pub block_max_deploy_count: u32,
    /// Maximum number of native transfer deploys allowed in a block (one of several block limits).
    pub block_max_transfer_count: u32,
    /// Maximum number of approvals (signatures) allowed in a block across all standard and
    /// transfer deploys (one of several block limits).
    pub block_max_approval_count: u32,
    /// Maximum sum of payment across all deploys included in a single block (one of several block
    /// limits).
    pub block_gas_limit: u64,
    /// Maximum length in bytes of payment args per deploy.
    pub payment_args_max_length: u32,
    /// Maximum length in bytes of session args per deploy.
    pub session_args_max_length: u32,
    /// Minimum token amount for a native transfer deploy (a transfer deploy received with an
    /// transfer amount less than this will be rejected upon receipt).
    pub native_transfer_minimum_motes: u64,
}

#[cfg(any(feature = "testing", test))]
impl DeployConfig {
    /// Generates a random instance using a `TestRng`.
    #[allow(deprecated)]
    pub fn random(rng: &mut TestRng) -> Self {
        let max_payment_cost = Motes::new(U512::from(rng.gen_range(1_000_000..1_000_000_000)));
        let max_ttl = TimeDiff::from_seconds(rng.gen_range(60..3_600));
        let max_dependencies = rng.gen();
        let max_block_size = rng.gen_range(1_000_000..1_000_000_000);
        let max_deploy_size = rng.gen_range(100_000..1_000_000);
        let block_max_deploy_count = rng.gen();
        let block_max_transfer_count = rng.gen();
        let block_max_approval_count = rng.gen();
        let block_gas_limit = rng.gen_range(100_000_000_000..1_000_000_000_000_000);
        let payment_args_max_length = rng.gen();
        let session_args_max_length = rng.gen();
        let native_transfer_minimum_motes =
            rng.gen_range(MAX_PAYMENT_AMOUNT..1_000_000_000_000_000);

        DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            max_deploy_size,
            block_max_deploy_count,
            block_max_transfer_count,
            block_max_approval_count,
            block_gas_limit,
            payment_args_max_length,
            session_args_max_length,
            native_transfer_minimum_motes,
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Default for DeployConfig {
    #[allow(deprecated)]
    fn default() -> Self {
        let one_day = TimeDiff::from_seconds(86400);
        // TimeDiff::from_str("1day").unwrap() doesn't work in testing feature scope
        DeployConfig {
            max_payment_cost: Motes::new(U512::zero()),
            max_ttl: one_day,
            max_dependencies: 10,
            max_block_size: 10_485_760,
            max_deploy_size: 1_048_576,
            block_max_deploy_count: 10,
            block_max_transfer_count: 1000,
            block_max_approval_count: 2600,
            block_gas_limit: 10_000_000_000_000,
            payment_args_max_length: 1024,
            session_args_max_length: 1024,
            native_transfer_minimum_motes: MAX_PAYMENT_AMOUNT,
        }
    }
}

impl ToBytes for DeployConfig {
    #[allow(deprecated)]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.max_payment_cost.value().to_bytes()?);
        buffer.extend(self.max_ttl.to_bytes()?);
        buffer.extend(self.max_dependencies.to_bytes()?);
        buffer.extend(self.max_block_size.to_bytes()?);
        buffer.extend(self.max_deploy_size.to_bytes()?);
        buffer.extend(self.block_max_deploy_count.to_bytes()?);
        buffer.extend(self.block_max_transfer_count.to_bytes()?);
        buffer.extend(self.block_max_approval_count.to_bytes()?);
        buffer.extend(self.block_gas_limit.to_bytes()?);
        buffer.extend(self.payment_args_max_length.to_bytes()?);
        buffer.extend(self.session_args_max_length.to_bytes()?);
        buffer.extend(self.native_transfer_minimum_motes.to_bytes()?);
        Ok(buffer)
    }

    #[allow(deprecated)]
    fn serialized_length(&self) -> usize {
        self.max_payment_cost.value().serialized_length()
            + self.max_ttl.serialized_length()
            + self.max_dependencies.serialized_length()
            + self.max_block_size.serialized_length()
            + self.max_deploy_size.serialized_length()
            + self.block_max_deploy_count.serialized_length()
            + self.block_max_transfer_count.serialized_length()
            + self.block_max_approval_count.serialized_length()
            + self.block_gas_limit.serialized_length()
            + self.payment_args_max_length.serialized_length()
            + self.session_args_max_length.serialized_length()
            + self.native_transfer_minimum_motes.serialized_length()
    }
}

impl FromBytes for DeployConfig {
    #[allow(deprecated)]
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_payment_cost, remainder) = U512::from_bytes(bytes)?;
        let max_payment_cost = Motes::new(max_payment_cost);
        let (max_ttl, remainder) = TimeDiff::from_bytes(remainder)?;
        let (max_dependencies, remainder) = u8::from_bytes(remainder)?;
        let (max_block_size, remainder) = u32::from_bytes(remainder)?;
        let (max_deploy_size, remainder) = u32::from_bytes(remainder)?;
        let (block_max_deploy_count, remainder) = u32::from_bytes(remainder)?;
        let (block_max_transfer_count, remainder) = u32::from_bytes(remainder)?;
        let (block_max_approval_count, remainder) = u32::from_bytes(remainder)?;
        let (block_gas_limit, remainder) = u64::from_bytes(remainder)?;
        let (payment_args_max_length, remainder) = u32::from_bytes(remainder)?;
        let (session_args_max_length, remainder) = u32::from_bytes(remainder)?;
        let (native_transfer_minimum_motes, remainder) = u64::from_bytes(remainder)?;
        let config = DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            max_deploy_size,
            block_max_deploy_count,
            block_max_transfer_count,
            block_max_approval_count,
            block_gas_limit,
            payment_args_max_length,
            session_args_max_length,
            native_transfer_minimum_motes,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::from_entropy();
        let config = DeployConfig::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }
}
