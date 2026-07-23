//! Public execution request types.

use casper_types::{evm, EvmConfig, EvmTransaction, U256};

use crate::{tx, Error};

/// Request passed to [`crate::EvmExecutor::execute`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecuteRequest {
    /// Block context available to EVM opcodes and validation.
    pub block: BlockContext,
    /// EVM work item to execute.
    pub kind: ExecuteKind,
}

/// Request passed to [`crate::EvmExecutor::execute_system_call`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemCallRequest {
    /// Block context available to EVM opcodes.
    pub block: BlockContext,
    /// System contract address.
    pub target: evm::Address,
    /// System call input bytes.
    pub input: Vec<u8>,
}

/// EVM work item to execute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecuteKind {
    /// Signed Ethereum transaction decoded by `casper-types`.
    Transaction(Box<EvmTransaction>),
    /// Unsigned local call request.
    Call(CallRequest),
}

/// Unsigned EVM call request.
///
/// Calls are useful for views, simulations, tests, and controlled system
/// execution. They still write effects into the supplied tracking copy, so
/// callers should pass a fork when they want to discard the result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallRequest {
    /// EVM address used as `msg.sender`.
    pub from: evm::Address,
    /// Target account, or `None` for contract creation.
    pub to: Option<evm::Address>,
    /// Amount of wei to send, encoded as a big-endian 256-bit word.
    pub value: U256,
    /// Calldata or contract init code.
    pub input: Vec<u8>,
    /// Gas available for execution.
    pub gas_limit: u64,
    /// Gas price used by gas-price-sensitive contracts.
    pub gas_price: u128,
    /// Nonce presented to revm when nonce checks are enabled.
    pub nonce: u64,
    /// Validation mode used for this unsigned call.
    pub validation: CallValidation,
}

/// Validation mode for unsigned EVM calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallValidation {
    /// Enforce EVM balance, nonce, chain-id, base-fee, and block-gas-limit checks.
    Checked,
    /// Disable EVM transaction validation checks for local simulations or controlled tests.
    UncheckedSimulation,
}

impl CallValidation {
    pub(crate) fn is_unchecked_simulation(self) -> bool {
        matches!(self, CallValidation::UncheckedSimulation)
    }
}

/// Per-execution block context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockContext {
    /// EVM block number.
    pub number: u64,
    /// EVM block timestamp in seconds since the Unix epoch.
    pub timestamp: u64,
    /// Block beneficiary address.
    pub beneficiary: evm::Address,
    /// Optional gas limit override. Defaults to chainspec `[evm].block_gas_limit`.
    pub gas_limit: Option<u64>,
    /// Optional base-fee override in wei.
    ///
    /// Defaults to chainspec `[evm].base_fee * [evm].wei_per_mote`.
    pub base_fee: Option<u128>,
    pub prevrandao: evm::Hash,
}

impl BlockContext {
    pub(crate) fn to_revm_block(
        &self,
        config: &EvmConfig,
    ) -> Result<revm::context::BlockEnv, Error> {
        let base_fee = self.base_fee.unwrap_or_else(|| config.base_fee_wei());
        let basefee = u64::try_from(base_fee).map_err(|_| {
            Error::Transaction("configured EVM base fee overflows revm u64".to_string())
        })?;
        Ok(revm::context::BlockEnv {
            number: revm::primitives::U256::from(self.number),
            beneficiary: tx::to_revm_address(self.beneficiary),
            timestamp: revm::primitives::U256::from(self.timestamp),
            gas_limit: self.gas_limit.unwrap_or(config.block_gas_limit),
            basefee,
            prevrandao: Some(tx::to_revm_hash(self.prevrandao)),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use casper_types::DEFAULT_WEI_PER_MOTE;

    use super::*;

    #[test]
    fn should_use_wei_denominated_base_fee_for_revm_block() {
        let config = EvmConfig {
            base_fee: 3,
            wei_per_mote: DEFAULT_WEI_PER_MOTE,
            ..Default::default()
        };
        let context = BlockContext {
            number: 1,
            timestamp: 1,
            beneficiary: evm::Address::ZERO,
            gas_limit: None,
            base_fee: None,
            prevrandao: evm::Hash::new([0x42; evm::HASH_LENGTH]),
        };
        let block = context
            .to_revm_block(&config)
            .expect("base fee should fit in revm block context");

        assert_eq!(u128::from(block.basefee), config.base_fee_wei());
        assert_ne!(block.basefee, config.base_fee);
        assert_eq!(
            block.prevrandao,
            Some(revm::primitives::B256::from([0x42; evm::HASH_LENGTH]))
        );
    }
}
