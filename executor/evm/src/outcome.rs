//! Public execution outcome types.

use casper_types::{evm, U512};
use revm::context_interface::result::{
    ExecutionResult, HaltReason as RevmHaltReason, OutOfGasError as RevmOutOfGasError, Output,
};

use crate::{state::BalanceLosses, tx, Error, Result};

/// Result returned by [`crate::EvmExecutor::execute`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionOutcome {
    /// High-level EVM execution status.
    pub status: ExecutionStatus,
    /// Gas consumed by execution.
    pub gas_used: u64,
    /// Return or revert bytes.
    pub output: Vec<u8>,
    /// Logs emitted by successful execution.
    pub logs: Vec<evm::Log>,
    /// Address created by a successful create transaction.
    pub created_contract_address: Option<evm::Address>,
    /// Wei lost through EVM burns, including deletion of selfdestructed purses.
    evm_burn_wei: U512,
    /// Wei discarded when final purse balances are rounded down for persistence.
    rounding_loss_wei: U512,
    /// Configured conversion rate for the combined balance loss.
    wei_per_mote: u64,
}

impl ExecutionOutcome {
    pub(crate) fn from_revm_result(
        result: &ExecutionResult,
        balance_losses: BalanceLosses,
        wei_per_mote: u64,
    ) -> Self {
        match result {
            ExecutionResult::Success {
                gas, logs, output, ..
            } => {
                let (output_bytes, created_contract_address) = match output {
                    Output::Call(bytes) => (bytes.to_vec(), None),
                    Output::Create(bytes, address) => {
                        (bytes.to_vec(), address.map(tx::from_revm_address))
                    }
                };
                Self {
                    status: ExecutionStatus::Success,
                    gas_used: gas.tx_gas_used(),
                    output: output_bytes,
                    logs: logs.iter().map(from_revm_log).collect(),
                    created_contract_address,
                    evm_burn_wei: balance_losses.evm_burn_wei,
                    rounding_loss_wei: balance_losses.rounding_loss_wei,
                    wei_per_mote,
                }
            }
            ExecutionResult::Revert { gas, output, .. } => Self {
                status: ExecutionStatus::Revert,
                gas_used: gas.tx_gas_used(),
                output: output.to_vec(),
                logs: Vec::new(),
                created_contract_address: None,
                evm_burn_wei: balance_losses.evm_burn_wei,
                rounding_loss_wei: balance_losses.rounding_loss_wei,
                wei_per_mote,
            },
            ExecutionResult::Halt { gas, reason, .. } => Self {
                status: ExecutionStatus::Halt(from_revm_halt_reason(reason)),
                gas_used: gas.tx_gas_used(),
                output: Vec::new(),
                logs: Vec::new(),
                created_contract_address: None,
                evm_burn_wei: balance_losses.evm_burn_wei,
                rounding_loss_wei: balance_losses.rounding_loss_wei,
                wei_per_mote,
            },
        }
    }

    /// Whole motes lost through EVM burns and balance rounding.
    ///
    /// The components are combined in wei before conversion because either
    /// component may contain a fraction of a mote. Returns an error if their
    /// sum overflows, the configured rate is zero, or the total is not a whole
    /// number of motes.
    pub fn supply_reduction_motes(&self) -> Result<U512> {
        let total_loss_wei = self
            .evm_burn_wei
            .checked_add(self.rounding_loss_wei)
            .ok_or_else(|| {
                Error::State("aggregate EVM balance loss overflowed U512 wei".to_string())
            })?;
        let wei_per_mote = U512::from(self.wei_per_mote);
        let remainder = total_loss_wei
            .checked_rem(wei_per_mote)
            .ok_or(Error::InvalidWeiPerMote)?;
        if !remainder.is_zero() {
            return Err(Error::State(format!(
                "aggregate EVM balance loss {total_loss_wei} wei is not divisible by \
                 {wei_per_mote} wei per mote"
            )));
        }
        total_loss_wei
            .checked_div(wei_per_mote)
            .ok_or(Error::InvalidWeiPerMote)
    }

    /// Converts this execution outcome into EVM receipt data.
    pub fn to_receipt(&self, effective_gas_price: u128) -> evm::Receipt {
        let status = match self.status {
            ExecutionStatus::Success => evm::ReceiptStatus::Success,
            ExecutionStatus::Revert => evm::ReceiptStatus::Revert,
            ExecutionStatus::Halt(reason) => evm::ReceiptStatus::Halt(reason),
        };
        evm::Receipt {
            status,
            gas_used: self.gas_used,
            effective_gas_price,
            contract_address: self.created_contract_address,
            logs: self.logs.clone(),
        }
    }
}

/// High-level EVM execution status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionStatus {
    /// Execution completed successfully.
    Success,
    /// Execution reverted and returned revert bytes.
    Revert,
    /// Execution halted, usually consuming all supplied gas.
    Halt(evm::HaltReason),
}

fn from_revm_halt_reason(reason: &RevmHaltReason) -> evm::HaltReason {
    match reason {
        RevmHaltReason::OutOfGas(reason) => {
            evm::HaltReason::OutOfGas(from_revm_out_of_gas_error(*reason))
        }
        RevmHaltReason::OpcodeNotFound => evm::HaltReason::OpcodeNotFound,
        RevmHaltReason::InvalidFEOpcode => evm::HaltReason::InvalidFEOpcode,
        RevmHaltReason::InvalidJump => evm::HaltReason::InvalidJump,
        RevmHaltReason::NotActivated => evm::HaltReason::NotActivated,
        RevmHaltReason::StackUnderflow => evm::HaltReason::StackUnderflow,
        RevmHaltReason::StackOverflow => evm::HaltReason::StackOverflow,
        RevmHaltReason::OutOfOffset => evm::HaltReason::OutOfOffset,
        RevmHaltReason::CreateCollision => evm::HaltReason::CreateCollision,
        RevmHaltReason::PrecompileError | RevmHaltReason::PrecompileErrorWithContext(_) => {
            evm::HaltReason::PrecompileError
        }
        RevmHaltReason::NonceOverflow => evm::HaltReason::NonceOverflow,
        RevmHaltReason::CreateContractSizeLimit => evm::HaltReason::CreateContractSizeLimit,
        RevmHaltReason::CreateContractStartingWithEF => {
            evm::HaltReason::CreateContractStartingWithEF
        }
        RevmHaltReason::CreateInitCodeSizeLimit => evm::HaltReason::CreateInitCodeSizeLimit,
        RevmHaltReason::OverflowPayment => evm::HaltReason::OverflowPayment,
        RevmHaltReason::StateChangeDuringStaticCall => evm::HaltReason::StateChangeDuringStaticCall,
        RevmHaltReason::CallNotAllowedInsideStatic => evm::HaltReason::CallNotAllowedInsideStatic,
        RevmHaltReason::OutOfFunds => evm::HaltReason::OutOfFunds,
        RevmHaltReason::CallTooDeep => evm::HaltReason::CallTooDeep,
    }
}

fn from_revm_out_of_gas_error(error: RevmOutOfGasError) -> evm::OutOfGasError {
    match error {
        RevmOutOfGasError::Basic => evm::OutOfGasError::Basic,
        RevmOutOfGasError::MemoryLimit => evm::OutOfGasError::MemoryLimit,
        RevmOutOfGasError::Memory => evm::OutOfGasError::Memory,
        RevmOutOfGasError::Precompile => evm::OutOfGasError::Precompile,
        RevmOutOfGasError::InvalidOperand => evm::OutOfGasError::InvalidOperand,
        RevmOutOfGasError::ReentrancySentry => evm::OutOfGasError::ReentrancySentry,
    }
}

fn from_revm_log(log: &revm::primitives::Log) -> evm::Log {
    evm::Log {
        address: tx::from_revm_address(log.address),
        topics: log
            .data
            .topics()
            .iter()
            .copied()
            .map(tx::from_revm_topic)
            .collect(),
        data: log.data.data.to_vec().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome_with_losses(
        evm_burn_wei: U512,
        rounding_loss_wei: U512,
        wei_per_mote: u64,
    ) -> ExecutionOutcome {
        ExecutionOutcome {
            status: ExecutionStatus::Success,
            gas_used: 0,
            output: Vec::new(),
            logs: Vec::new(),
            created_contract_address: None,
            evm_burn_wei,
            rounding_loss_wei,
            wei_per_mote,
        }
    }

    #[test]
    fn supply_reduction_combines_fractional_losses_before_converting_at_configured_rate() {
        let outcome = outcome_with_losses(U512::from(9u64), U512::one(), 10);

        assert_eq!(outcome.supply_reduction_motes().unwrap(), U512::one());
    }

    #[test]
    fn supply_reduction_accepts_maximum_whole_mote_loss() {
        let outcome = outcome_with_losses(U512::MAX, U512::zero(), 1);

        assert_eq!(outcome.supply_reduction_motes().unwrap(), U512::MAX);
    }

    #[test]
    fn supply_reduction_rejects_loss_overflow() {
        let outcome = outcome_with_losses(U512::MAX, U512::one(), 1);

        assert!(matches!(
            outcome.supply_reduction_motes(),
            Err(Error::State(message)) if message.contains("balance loss overflowed U512 wei")
        ));
    }

    #[test]
    fn supply_reduction_rejects_zero_conversion_rate() {
        let outcome = outcome_with_losses(U512::zero(), U512::zero(), 0);

        assert!(matches!(
            outcome.supply_reduction_motes(),
            Err(Error::InvalidWeiPerMote)
        ));
    }

    #[test]
    fn supply_reduction_rejects_fractional_mote_loss() {
        let outcome = outcome_with_losses(U512::from(9u64), U512::zero(), 10);

        assert!(matches!(
            outcome.supply_reduction_motes(),
            Err(Error::State(message))
                if message.contains("balance loss 9 wei is not divisible by 10 wei per mote")
        ));
    }
}
