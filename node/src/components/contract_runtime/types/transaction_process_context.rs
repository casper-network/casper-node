use crate::{
    contract_runtime::{
        types::{EvmOriginResolution, ExecutionArtifact, LimitsAndCosts, ProcessRequest},
        StateResultError,
    },
    types::{
        transaction::{WasmV2Error, WasmV2InvalidRequest, WasmV2Result},
        MetaTransaction, TransactionHeader,
    },
};
use casper_execution_engine::engine_state::{
    Error, InvalidRequest as InvalidWasmV1Request, WasmV1Result,
};
use casper_executor_evm::ExecutionStatus;
use casper_storage::data_access_layer::{
    auction::AuctionMethodError, mint::BurnResult, BalanceHoldResult, BalanceIdentifier,
    BalanceIdentifierError, BalanceResult, BiddingResult, HandleFeeResult, HandleRefundResult,
    TransferResult,
};
use casper_types::{
    account::AccountHash,
    contract_messages::Messages,
    evm,
    evm::Receipt as EvmReceipt,
    execution::{Effects, EvmExecutionResult, ExecutionResult, ExecutionResultV2},
    Chainspec, EvmTransactionError, Gas, HashAddr, InitiatorAddr, InvalidTransaction, PublicKey,
    Transaction, TransactionHash, Transfer, AUCTION_LANE_ID, MINT_LANE_ID, U512,
};
use std::collections::BTreeSet;
use strum::Display;

pub(crate) enum BalanceIdentifierResolution {
    Identifier(BalanceIdentifier),
    CheckContractPay(BalanceIdentifier),
    CheckEvmAccount(BalanceIdentifier),
}

pub(crate) enum InitialBalanceIdentifierResult {
    Unknown,
    Identifier(BalanceIdentifier),
    IdentifierAndStateError(BalanceIdentifier, StateResultError),
    IdentifierAndEvmResolution(BalanceIdentifier, EvmOriginResolution),
    IdentifierAndEvmError(BalanceIdentifier, EvmTransactionError),
}

impl InitialBalanceIdentifierResult {
    pub(crate) fn balance_identifier(&self) -> Option<&BalanceIdentifier> {
        match self {
            InitialBalanceIdentifierResult::Unknown => None,
            InitialBalanceIdentifierResult::Identifier(bi)
            | InitialBalanceIdentifierResult::IdentifierAndStateError(bi, _)
            | InitialBalanceIdentifierResult::IdentifierAndEvmResolution(bi, _)
            | InitialBalanceIdentifierResult::IdentifierAndEvmError(bi, _) => Some(bi),
        }
    }

    pub(crate) fn state_result_error(&self) -> Option<&StateResultError> {
        if let InitialBalanceIdentifierResult::IdentifierAndStateError(_, err) = self {
            Some(err)
        } else {
            None
        }
    }

    pub(crate) fn evm_origin_resolution(&self) -> Option<&EvmOriginResolution> {
        if let InitialBalanceIdentifierResult::IdentifierAndEvmResolution(_, eor) = self {
            Some(eor)
        } else {
            None
        }
    }

    pub(crate) fn evm_transaction_error(&self) -> Option<&EvmTransactionError> {
        if let InitialBalanceIdentifierResult::IdentifierAndEvmError(_, err) = self {
            Some(err)
        } else {
            None
        }
    }
}

#[derive(Debug, Display)]
#[allow(unused)] // TODO remove this when ready
pub(crate) enum TransactionProcessContextError {
    InvalidTransaction(InvalidTransaction),
    MissingInitiatorAddr,
    MissingHeader,
    MissingEvmGasLimit,
    MissingMetaTransaction,
    MissingCostEstimate,
    FailedToSetCost,
    FailedToSetEvmCost,
    EvmFeeOverflow,
    InvalidEvmTransaction,
    BalanceIdentifierError(Box<BalanceIdentifierError>),
}

#[derive(Clone, Debug)]
pub(crate) struct TransactionProcessContext {
    transaction_hash: TransactionHash,
    header: TransactionHeader,
    meta_transaction: MetaTransaction,
    limits_and_costs: LimitsAndCosts,

    initial_balance_identifier: Option<BalanceIdentifier>,
    initial_balance_result: Option<BalanceResult>,

    evm_origin_resolution: Option<EvmOriginResolution>,
    evm_receipt: Option<EvmReceipt>,

    exec_attempted: bool,
    error_message: Option<String>,
    effects: Effects,
    messages: Messages,
    transfers: Vec<Transfer>,
}

impl TransactionProcessContext {
    pub(crate) fn try_new(
        txn: &Transaction,
        chainspec: &Chainspec,
        gas_price: u8,
    ) -> Result<Self, TransactionProcessContextError> {
        let meta_transaction =
            match MetaTransaction::new_from_txn_with_price(txn, chainspec, gas_price) {
                Ok(meta_transaction) => meta_transaction,
                Err(err) => return Err(TransactionProcessContextError::InvalidTransaction(err)),
            };

        let limits_and_costs = LimitsAndCosts::try_from_meta_txn(&meta_transaction, chainspec)?;

        Ok(TransactionProcessContext {
            effects: Effects::new(),
            transaction_hash: txn.hash(),
            header: txn.into(),
            meta_transaction,
            limits_and_costs,

            initial_balance_identifier: None,
            initial_balance_result: None,
            evm_receipt: None,
            evm_origin_resolution: None,

            exec_attempted: false,
            error_message: None,
            transfers: vec![],
            messages: Default::default(),
        })
    }

    pub(crate) fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    pub(crate) fn initiator_addr(&self) -> InitiatorAddr {
        self.meta_transaction.initiator_addr().clone()
    }

    pub(crate) fn initial_balance_identifier(&self) -> Option<&BalanceIdentifier> {
        self.initial_balance_identifier.as_ref()
    }

    pub(crate) fn contract_direct_address(&self) -> Option<(HashAddr, String)> {
        self.meta_transaction.contract_direct_address()
    }

    pub(crate) fn authorization_keys(&self) -> BTreeSet<AccountHash> {
        self.meta_transaction.authorization_keys()
    }

    pub(crate) fn gas_price(&self) -> u8 {
        self.limits_and_costs.gas_price()
    }

    pub(crate) fn gas_limit(&self) -> Gas {
        let is_evm = self.meta_transaction.is_evm();
        if is_evm {
            Gas::new(self.cost_to_use())
        } else {
            self.limits_and_costs.gas_limit()
        }
    }

    pub(crate) fn consumed(&self) -> U512 {
        if self.error_message.is_some() {
            return self.cost_to_use();
        }
        match &self.initial_balance_identifier {
            Some(bi) => {
                if bi.is_penalty() {
                    return self.cost_to_use();
                }
            }
            None => return U512::zero(), // can't consume if no purse
        }
        self.limits_and_costs.consumed().unwrap_or_default().value()
    }

    pub(crate) fn available(&self) -> Option<U512> {
        self.limits_and_costs.available()
    }

    pub(crate) fn refund_amount(&self) -> U512 {
        self.limits_and_costs.refund()
    }

    pub(crate) fn has_sufficient_minimum(&self) -> bool {
        self.has_sufficient_balance(self.limits_and_costs.min_cost())
    }

    pub(crate) fn has_sufficient_estimated(&self) -> bool {
        self.has_sufficient_balance(self.limits_and_costs.cost_estimate())
    }

    pub(crate) fn has_sufficient_balance(&self, amount: U512) -> bool {
        match self.limits_and_costs.available() {
            Some(available) => available >= amount,
            None => false,
        }
    }

    pub(crate) fn cost_to_use(&self) -> U512 {
        // to prevent do-nothing exhaustion and other 0 cost scenarios,
        // we raise cost to min_cost if less than that

        let initial_cost = self.limits_and_costs.initial_cost();
        let min_cost = self.limits_and_costs.min_cost();

        let cost = {
            let cost = initial_cost;
            if cost < min_cost {
                min_cost
            } else {
                cost
            }
        };

        match self.limits_and_costs.available() {
            Some(available) => {
                if available < initial_cost {
                    available
                } else {
                    cost
                }
            }
            None => cost,
        }
    }

    pub(crate) fn fee_amount(&self) -> U512 {
        let cost_to_use = self.cost_to_use();
        let refund_amount = self.refund_amount();
        let available = self.available().unwrap_or(U512::zero());
        // take the lower of the difference between cost - refund OR available
        cost_to_use.saturating_sub(refund_amount).min(available)
    }

    pub(crate) fn transaction_lane(&self) -> u8 {
        self.meta_transaction.transaction_lane()
    }

    pub(crate) fn error_message(&self) -> Option<String> {
        self.error_message.clone()
    }
}

impl TransactionProcessContext {
    // *************** EVM **************

    pub(crate) fn evm_effective_gas_price(&self) -> Option<u128> {
        self.meta_transaction
            .evm_effective_gas_cost(self.limits_and_costs.base_fee_wei())
    }

    pub(crate) fn evm_address(&self) -> Option<evm::Address> {
        if let Some(addr) = &self.initiator_addr().evm_address() {
            return Some(*addr);
        }
        None
    }

    pub(crate) fn evm_addr_receipt(&self) -> Option<(evm::Address, EvmReceipt)> {
        if let Some(addr) = self.evm_address() {
            if let Some(receipt) = &self.evm_receipt {
                return Some((addr, receipt.clone()));
            }
        }
        None
    }

    pub(crate) fn evm_signer(&self) -> Option<Result<&PublicKey, EvmTransactionError>> {
        self.meta_transaction.evm_signer()
    }
}

impl TransactionProcessContext {
    // *************** APPENDERS (aka "WITHS") **************
    pub(crate) fn with_gas_limit_consumed(&mut self) -> &mut Self {
        self.limits_and_costs.with_gas_limit_consumed();
        self
    }

    pub(crate) fn with_added_consumed(&mut self, consumed: Gas) -> &mut Self {
        self.limits_and_costs.with_added_consumed(consumed);
        self
    }

    pub(crate) fn with_appended_transfers(&mut self, transfers: &mut Vec<Transfer>) -> &mut Self {
        self.transfers.append(transfers);
        self
    }

    pub(crate) fn with_appended_effects(&mut self, effects: Effects) -> &mut Self {
        self.effects.append(effects);
        self
    }

    pub(crate) fn with_appended_messages(&mut self, messages: &mut Messages) -> &mut Self {
        self.messages.append(messages);
        self
    }

    pub(crate) fn with_state_result_error(
        &mut self,
        error: StateResultError,
    ) -> Result<&mut Self, ()> {
        if let StateResultError::RootNotFound = error {
            return Err(());
        }
        if self.error_message.is_none() {
            self.error_message = Some(format!("{:?}", error));
        }
        Ok(self)
    }

    pub(crate) fn with_evm_error(&mut self, error: EvmTransactionError) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{:?}", error));
        }
        self
    }

    pub(crate) fn with_initial_balance_identifier(
        &mut self,
        identifier: BalanceIdentifier,
    ) -> &mut Self {
        self.initial_balance_identifier = Some(identifier);
        self
    }

    pub(crate) fn with_initial_balance_result(
        &mut self,
        balance_result: BalanceResult,
    ) -> &mut Self {
        // there is no point recording BalanceResult::RootNotFound because it is unrecoverable
        if let (None, Some(err)) = (&self.error_message, balance_result.error()) {
            self.error_message = Some(format!("{}", err));
        }
        if let Some(purse) = balance_result.purse_addr() {
            let minimum_amount = self.limits_and_costs.min_cost();
            let is_sufficient = balance_result.is_sufficient(minimum_amount);
            if !is_sufficient {
                self.error_message = Some(format!(
                    "Purse {} has less than minimum amount {}",
                    base16::encode_lower(&purse),
                    minimum_amount
                ));
            }
        }
        let available = balance_result.available_balance().copied();
        self.limits_and_costs.with_available(available);
        self.initial_balance_result = Some(balance_result);
        self
    }

    pub(crate) fn with_wasm_v1_result(
        &mut self,
        wasm_v1_result: WasmV1Result,
    ) -> Result<&mut Self, ()> {
        if let Some(Error::RootNotFound(_)) = wasm_v1_result.error() {
            return Err(());
        }
        self.with_added_consumed(wasm_v1_result.consumed());

        if let Some(err) = wasm_v1_result.error() {
            self.error_message = Some(format!("{}", err));
        } else if wasm_v1_result.consumed() == Gas::zero() {
            self.error_message = Some("Wasm consumed 0 gas".to_string());
        }

        if self.error_message.is_some() {
            return Ok(self);
        }

        self.with_appended_transfers(&mut wasm_v1_result.transfers().clone())
            .with_appended_messages(&mut wasm_v1_result.messages().clone())
            .with_appended_effects(wasm_v1_result.effects().clone());
        Ok(self)
    }

    pub(crate) fn with_error_message(&mut self, error_message: String) -> &mut Self {
        self.error_message = Some(error_message);
        self
    }

    pub(crate) fn with_handle_refund_result(
        &mut self,
        handle_refund_result: &HandleRefundResult,
    ) -> Result<&mut Self, ()> {
        if let HandleRefundResult::RootNotFound = handle_refund_result {
            return Err(());
        }
        if let HandleRefundResult::Success {
            effects, transfers, ..
        } = handle_refund_result
        {
            self.with_appended_transfers(&mut transfers.clone())
                .with_appended_effects(effects.clone());
        }
        if let (None, HandleRefundResult::Failure(_)) = (&self.error_message, handle_refund_result)
        {
            self.error_message = handle_refund_result.error_message();
            return Ok(self);
        }
        Ok(self)
    }

    pub(crate) fn with_handle_fee_result(
        &mut self,
        handle_fee_result: &HandleFeeResult,
    ) -> Result<&mut Self, ()> {
        if let HandleFeeResult::RootNotFound = handle_fee_result {
            return Err(());
        }
        if let HandleFeeResult::Success {
            effects, transfers, ..
        } = handle_fee_result
        {
            self.with_appended_transfers(&mut transfers.clone())
                .with_appended_effects(effects.clone());
        }
        if let (None, HandleFeeResult::Failure(_)) = (&self.error_message, handle_fee_result) {
            self.error_message = handle_fee_result.error_message();
            return Ok(self);
        }
        Ok(self)
    }

    pub(crate) fn with_balance_hold_result(
        &mut self,
        hold_result: &BalanceHoldResult,
    ) -> Result<&mut Self, ()> {
        if let BalanceHoldResult::RootNotFound = hold_result {
            return Err(());
        }
        if let BalanceHoldResult::Success { effects, .. } = hold_result {
            self.with_appended_effects(*effects.clone());
        }
        if let (None, BalanceHoldResult::Failure(_)) = (&self.error_message, &hold_result) {
            self.error_message = hold_result.error_message();
            return Ok(self);
        }
        Ok(self)
    }

    pub(crate) fn with_refund_amount(&mut self, refund: U512) -> &mut Self {
        self.limits_and_costs.with_refund(refund);
        self
    }

    pub(crate) fn with_zero_cost(&mut self) -> &mut Self {
        self.limits_and_costs.with_zero_cost();
        self
    }

    pub(crate) fn with_evm_execution_outcome(
        &mut self,
        outcome: casper_executor_evm::ExecutionOutcome,
        effective_gas_price: u128,
        effects: Effects,
    ) -> &mut Self {
        let consumed = match outcome.status {
            ExecutionStatus::Success => {
                let gas_used = outcome.gas_used;
                self.limits_and_costs
                    .evm_gas_fee_motes(gas_used, effective_gas_price)
                    .unwrap_or(self.cost_to_use())
            }
            ExecutionStatus::Revert => self.cost_to_use(),
            ExecutionStatus::Halt(reason) => {
                if self.error_message.is_none() {
                    self.error_message = Some(format!("{:?}", reason));
                }
                self.cost_to_use()
            }
        };

        let receipt = outcome.to_receipt(effective_gas_price);
        self.evm_receipt = Some(receipt);
        self.limits_and_costs
            .with_added_consumed(Gas::new(consumed));
        self.with_appended_effects(effects);

        self

        // if let Some(err) = wasm_v1_result.error() {
        //     self.error_message = Some(format!("{}", err));
        // } else if wasm_v1_result.consumed() == Gas::zero() {
        //     self.error_message = Some("Wasm consumed 0 gas".to_string());
        // }
        //
        // if self.error_message.is_some() {
        //     return Ok(self);
        // }
        //
        // self.with_appended_transfers(&mut wasm_v1_result.transfers().clone())
        //     .with_appended_messages(&mut wasm_v1_result.messages().clone())
        //     .with_appended_effects(wasm_v1_result.effects().clone());
        // Ok(self)
    }

    pub(crate) fn with_invalid_wasm_v1_request(
        &mut self,
        invalid_request: &InvalidWasmV1Request,
    ) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{}", invalid_request));
        }
        self
    }

    pub(crate) fn with_auction_method_error(
        &mut self,
        auction_method_error: &AuctionMethodError,
    ) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{}", auction_method_error));
        }
        self
    }

    pub(crate) fn with_transfer_result(
        &mut self,
        transfer_result: TransferResult,
    ) -> Result<&mut Self, ()> {
        if let TransferResult::RootNotFound = transfer_result {
            return Err(());
        }
        if let (None, TransferResult::Failure(err)) = (&self.error_message, &transfer_result) {
            self.error_message = Some(format!("{}", err));
        }
        if let TransferResult::Success {
            effects,
            transfers,
            cache: _,
        } = transfer_result
        {
            self.with_appended_transfers(&mut transfers.clone())
                .with_appended_effects(effects);
        }
        Ok(self)
    }

    pub(crate) fn with_burn_result(&mut self, burn_result: BurnResult) -> Result<&mut Self, ()> {
        if let BurnResult::RootNotFound = burn_result {
            return Err(());
        }
        if let (None, BurnResult::Failure(err)) = (&self.error_message, &burn_result) {
            self.error_message = Some(format!("{}", err));
        }
        if let BurnResult::Success { effects, cache: _ } = burn_result {
            self.with_appended_effects(effects);
        }
        Ok(self)
    }

    pub(crate) fn with_bidding_result(
        &mut self,
        bidding_result: BiddingResult,
    ) -> Result<&mut Self, ()> {
        if let BiddingResult::RootNotFound = bidding_result {
            return Err(());
        }
        if let (None, BiddingResult::Failure(err)) = (&self.error_message, &bidding_result) {
            self.error_message = Some(format!("{}", err));
        }
        if let BiddingResult::Success {
            effects, transfers, ..
        } = bidding_result
        {
            self.with_appended_transfers(&mut transfers.clone())
                .with_appended_effects(effects);
        }
        Ok(self)
    }

    /// Adds the error message from a `InvalidRequest` to the artifact.
    pub(crate) fn with_invalid_wasm_v2_request(&mut self, ire: WasmV2InvalidRequest) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{}", ire));
        }
        self
    }

    /// Adds the result from a `WasmV2Result` to the artifact.
    pub(crate) fn with_wasm_v2_result(&mut self, result: WasmV2Result) -> &mut Self {
        self.with_added_consumed(Gas::from(result.gas_usage().gas_spent()));

        // TODO: Use system message to notify about contract hash

        self.with_appended_effects(result.effects().clone());

        self
    }

    /// Adds the error message from a `WasmV2Error` to the artifact.
    #[inline]
    pub(crate) fn with_wasm_v2_error(&mut self, error: WasmV2Error) -> &mut Self {
        self.with_error_message(error.to_string());
        self
    }

    pub(crate) fn with_exec_attempt(&mut self) -> &mut Self {
        self.exec_attempted = true;
        if self.error_message.is_some() {
            return self;
        }
        match &self.initial_balance_identifier {
            Some(bi) => {
                if bi.is_penalty() {
                    self.with_error_message("exec attempt while penalized".to_string());
                    return self;
                }
            }
            None => {
                let err_msg = "exec attempt without initial balance identifier".to_string();
                self.with_error_message(err_msg);
                return self;
            }
        }

        if !self.has_sufficient_estimated() {
            let err_msg = "exec attempt with less than estimated balance".to_string();
            self.with_error_message(err_msg);
            return self;
        }
        self
    }

    pub(crate) fn with_evm_origin_resolution(
        &mut self,
        resolution: Option<EvmOriginResolution>,
    ) -> &mut Self {
        self.evm_origin_resolution = resolution;
        self
    }
}

impl TransactionProcessContext {
    // *************** FLOW CONTROL ****************

    fn allow_execution(&self) -> bool {
        if self.error_message.is_some() {
            return false;
        }

        true
    }

    pub(crate) fn has_error(&self) -> bool {
        self.error_message.is_some()
    }

    pub(crate) fn process_request(&self) -> ProcessRequest<'_> {
        let txn = &self.meta_transaction;
        if !self.allow_execution() {
            let is_evm = txn.is_evm();
            if is_evm {
                let effective_gas_price = self.evm_effective_gas_price().unwrap_or(0u128);
                return ProcessRequest::NoExecEvm {
                    effective_gas_price,
                };
            }
            return ProcessRequest::NoExec;
        }

        let lane = txn.transaction_lane();
        if lane == MINT_LANE_ID {
            return ProcessRequest::NativeMint {
                session_args: txn.session_args(),
                entry_point: txn.entry_point(),
            };
        }
        if lane == AUCTION_LANE_ID {
            return ProcessRequest::NativeAuction {
                session_args: txn.session_args(),
                entry_point: txn.entry_point(),
            };
        }
        if txn.is_v1_wasm() {
            return ProcessRequest::WasmV1 {
                session_input_data: txn.to_session_input_data(),
            };
        }
        if txn.is_v2_wasm() {
            return ProcessRequest::WasmV2 {
                transaction_input: txn.to_transaction_info(),
            };
        }
        match txn.as_evm() {
            Some(evm_txn) => {
                let effective_gas_price = match self.evm_effective_gas_price() {
                    Some(effective_gas_price) => effective_gas_price,
                    None => return ProcessRequest::Unknown,
                };

                let block_gas_limit = self.limits_and_costs.evm_block_gas_limit();
                let base_fee_wei = self.limits_and_costs.base_fee_wei();

                ProcessRequest::EvmV1 {
                    evm_txn: evm_txn.clone(),
                    base_fee_wei,
                    effective_gas_price,
                    block_gas_limit,
                }
            }
            None => ProcessRequest::Unknown,
        }
    }

    pub(crate) fn balance_identifier_resolution(
        &self,
    ) -> Result<BalanceIdentifierResolution, TransactionProcessContextError> {
        let initiator_addr = self.initiator_addr().clone();
        let default_ret = match BalanceIdentifier::try_from(initiator_addr.clone()) {
            Ok(bi) => bi,
            Err(_) => {
                return Err(TransactionProcessContextError::BalanceIdentifierError(
                    Box::new(BalanceIdentifierError::TryFromInitiatorAddr(initiator_addr)),
                ))
            }
        };
        match &self.meta_transaction {
            MetaTransaction::Deploy(_) => Ok(BalanceIdentifierResolution::Identifier(default_ret)),
            MetaTransaction::V1(mv1) => {
                if mv1.is_v1_wasm() {
                    Ok(BalanceIdentifierResolution::CheckContractPay(default_ret))
                } else {
                    Ok(BalanceIdentifierResolution::Identifier(default_ret))
                }
            }
            MetaTransaction::Evm(mtxn) => {
                let evm_ret = match BalanceIdentifier::try_from(mtxn.initiator_addr()) {
                    Ok(bi) => bi,
                    Err(_) => {
                        return Err(TransactionProcessContextError::BalanceIdentifierError(
                            Box::new(BalanceIdentifierError::TryFromInitiatorAddr(initiator_addr)),
                        ))
                    }
                };

                Ok(BalanceIdentifierResolution::CheckEvmAccount(evm_ret))
            }
        }
    }

    // *************** TAKE ****************
    pub(crate) fn into_execution_artifact(self) -> ExecutionArtifact {
        let actual_cost = self.cost_to_use();
        let initiator_addr = self.initiator_addr().clone();
        let size_estimate = self.meta_transaction.size_estimate() as u64;
        let execution_result = if let Some((initiator, receipt)) = &self.evm_addr_receipt() {
            let result = EvmExecutionResult {
                initiator: *initiator,
                current_price: self.limits_and_costs.gas_price(),
                limit: self.limits_and_costs.gas_limit(),
                cost: actual_cost,
                refund: self.limits_and_costs.refund(),
                size_estimate,
                effects: self.effects,
                receipt: receipt.clone(),
            };
            ExecutionResult::from(result)
        } else {
            let result = ExecutionResultV2 {
                effects: self.effects,
                transfers: self.transfers,
                initiator: initiator_addr,
                refund: self.limits_and_costs.refund(),
                limit: self.limits_and_costs.gas_limit(),
                consumed: self.limits_and_costs.consumed().unwrap_or_default(),
                cost: actual_cost,
                current_price: self.limits_and_costs.gas_price(),
                size_estimate,
                error_message: self.error_message,
            };
            ExecutionResult::V2(Box::new(result))
        };
        ExecutionArtifact::new(
            self.transaction_hash,
            self.header.clone(),
            execution_result,
            self.messages,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::DEFAULT_WEI_PER_MOTE;

    #[test]
    fn should_convert_wei_gas_fee_to_motes() {
        let x = LimitsAndCosts::default();

        assert_eq!(
            x.evm_gas_fee_motes(21_000, u128::from(DEFAULT_WEI_PER_MOTE)),
            Some(U512::from(21_000))
        );
    }

    #[test]
    fn should_round_sub_mote_wei_gas_fee_up_to_one_mote() {
        let x = LimitsAndCosts::default();

        assert_eq!(x.evm_gas_fee_motes(1, 1), Some(U512::from(1)));
    }
}
