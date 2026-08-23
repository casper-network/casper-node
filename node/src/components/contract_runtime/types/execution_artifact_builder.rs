use crate::{
    contract_runtime::{
        types::{EvmOriginResolution, ExecutionArtifact},
        StateResultError,
    },
    types::{
        transaction::{WasmV2Error, WasmV2InvalidRequest, WasmV2Result, WasmV2TransactionInput},
        MetaTransaction, TransactionHeader,
    },
};
use casper_execution_engine::engine_state::{
    Error, InvalidRequest as InvalidWasmV1Request, SessionInputData, WasmV1Result,
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
    Chainspec, EvmTransaction, EvmTransactionError, Gas, HashAddr, InitiatorAddr,
    InvalidTransaction, PublicKey, Transaction, TransactionArgs, TransactionEntryPoint,
    TransactionHash, Transfer, AUCTION_LANE_ID, MINT_LANE_ID, U512,
};
use num_rational::Ratio;
use std::{borrow::Cow, collections::BTreeSet, fmt::Formatter};
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

pub(crate) enum ProcessRequest<'a> {
    Unknown,
    NativeMint {
        session_args: Cow<'a, TransactionArgs>,
        entry_point: TransactionEntryPoint,
    },
    NativeAuction {
        session_args: Cow<'a, TransactionArgs>,
        entry_point: TransactionEntryPoint,
    },
    WasmV1 {
        session_input_data: SessionInputData<'a>,
    },
    WasmV2 {
        transaction_info: WasmV2TransactionInput<'a>,
    },
    EvmV1 {
        evm_txn: EvmTransaction,
        base_fee_wei: u128,
        effective_gas_price: u128,
        block_gas_limit: u64,
    },
    NoExecEvm {
        effective_gas_price: u128,
    },
    NoExec,
}

impl<'a> std::fmt::Display for ProcessRequest<'a> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessRequest::Unknown => {
                write!(formatter, "unknown process request")
            }
            ProcessRequest::NativeMint { .. } => write!(formatter, "native mint process request"),
            ProcessRequest::NativeAuction { .. } => {
                write!(formatter, "native auction process request")
            }
            ProcessRequest::WasmV1 { .. } => write!(formatter, "wasm_v1 process request"),
            ProcessRequest::WasmV2 { .. } => write!(formatter, "wasm_v2 process request"),
            ProcessRequest::EvmV1 { .. } => write!(formatter, "evm_v1 process request"),
            ProcessRequest::NoExecEvm { .. } => write!(formatter, "no_exec_evm process request"),
            ProcessRequest::NoExec => write!(formatter, "no_exec process request"),
        }
    }
}

#[derive(Debug, Display)]
#[allow(unused)] // TODO remove this when ready
pub(crate) enum ExecutionArtifactBuilderError {
    InvalidTransaction(InvalidTransaction),
    MissingInitiatorAddr,
    MissingHeader,
    MissingEvmGasLimit,
    MissingMetaTransaction,
    FailedToSetCost,
    FailedToSetEvmCost,
    EvmFeeOverflow,
    InvalidEvmTransaction,
    BalanceIdentifierError(Box<BalanceIdentifierError>),
}

#[derive(Clone, Debug)]
struct LimitsAndCosts {
    gas_limit: Gas,
    initial_cost: U512,
    min_cost: U512,
    wei_per_mote: u128,
    base_fee_wei: u128,
    evm_block_gas_limit: u64,

    current_price: u8,
    consumed: Option<Gas>,
    refund: Option<U512>,
    available: Option<U512>,
}

impl LimitsAndCosts {
    pub(crate) fn new(
        gas_limit: Gas,
        initial_cost: U512,
        min_cost: U512,
        wei_per_mote: u128,
        base_fee_wei: u128,
        evm_block_gas_limit: u64,
    ) -> Self {
        LimitsAndCosts {
            gas_limit,
            initial_cost,
            min_cost,
            wei_per_mote,
            base_fee_wei,
            current_price: 0,
            consumed: None,
            refund: None,
            available: None,
            evm_block_gas_limit,
        }
    }

    pub(crate) fn evm_block_gas_limit(&self) -> u64 {
        self.evm_block_gas_limit
    }

    pub(crate) fn gas_limit(&self) -> Gas {
        self.gas_limit
    }

    pub(crate) fn initial_cost(&self) -> U512 {
        self.initial_cost
    }

    pub(crate) fn min_cost(&self) -> U512 {
        self.min_cost
    }

    pub(crate) fn current_price(&self) -> u8 {
        self.current_price
    }

    #[allow(unused)]
    pub(crate) fn with_current_price(&mut self, current_price: u8) -> &mut Self {
        self.current_price = current_price;
        self
    }

    pub(crate) fn consumed(&self) -> Gas {
        self.consumed.unwrap_or_default()
    }

    #[allow(unused)]
    pub(crate) fn with_consumed(&mut self, consumed: Gas) -> &mut Self {
        self.consumed = Some(consumed);
        self
    }

    pub(crate) fn with_gas_limit_consumed(&mut self) -> &mut Self {
        match self.consumed {
            Some(consumed) => {
                self.consumed = Some(consumed.saturating_add(self.gas_limit));
            }
            None => {
                self.consumed = Some(self.gas_limit);
            }
        }
        self
    }

    pub(crate) fn with_added_consumed(&mut self, added_consumed: Gas) -> &mut Self {
        match self.consumed {
            Some(consumed) => {
                self.consumed = Some(consumed.saturating_add(added_consumed));
            }
            None => {
                self.consumed = Some(added_consumed);
            }
        }
        self
    }

    pub(crate) fn refund(&self) -> U512 {
        self.refund.unwrap_or_default()
    }

    pub(crate) fn with_refund(&mut self, refund: U512) -> &mut Self {
        self.refund = Some(refund);
        self
    }

    #[allow(unused)]
    pub(crate) fn available(&self) -> U512 {
        self.available.unwrap_or_default()
    }

    pub(crate) fn with_available(&mut self, available: Option<U512>) -> &mut Self {
        self.available = available;
        self
    }

    pub(crate) fn with_zero_cost(&mut self) -> &mut Self {
        self.initial_cost = U512::zero();
        self.min_cost = U512::zero();
        self
    }

    pub(crate) fn wei_per_mote(&self) -> u128 {
        self.wei_per_mote
    }

    pub(crate) fn base_fee_wei(&self) -> u128 {
        self.base_fee_wei
    }

    /// Converts an EVM gas cost, denominated in wei, to motes by rounding up.
    ///
    /// Rounding is applied after multiplying gas by price, so sub-mote totals
    /// are charged as one mote without overcharging each gas unit separately.
    pub(crate) fn evm_gas_fee_motes(&self, gas: u64, gas_price_wei: u128) -> Option<U512> {
        let wei_per_mote = self.wei_per_mote();
        if wei_per_mote == 0 {
            return None;
        }
        let fee_wei = U512::from(gas).checked_mul(U512::from(gas_price_wei))?;
        Some(
            Ratio::new(fee_wei, U512::from(wei_per_mote))
                .ceil()
                .to_integer(),
        )
    }

    fn try_from_meta_txn(
        mtxn: &MetaTransaction,
        chainspec: &Chainspec,
    ) -> Result<LimitsAndCosts, ExecutionArtifactBuilderError> {
        /*
        we solve for halting state using a `gas limit` which is the maximum amount of
        computation we will allow a given transaction to consume. the transaction itself
        provides a function to determine this if provided with the current cost tables
        gas_limit is ALWAYS calculated with price == 1.

        next there is the actual cost, i.e. how much we charge for that computation
        this is calculated by multiplying the gas limit by the current `gas_price`
        gas price has a floor of 1, and the ceiling is configured in the chainspec
        NOTE: when the gas price is 1, the gas limit and the cost are coincidentally
        equal because x == x * 1; thus it is recommended to run tests with
        price >1 to avoid being confused by this.

        the third important value is the amount of computation consumed by executing a transaction.
        consumed is determined after execution and is used for refund & fee post-processing.

        for native transactions, there is no wasm and the consumed always equals the limit.
        for bytecode / wasm based transactions, the consumed is based on what opcodes were executed
            and can range from >=0 to <=gas_limit.
        */

        let evm_block_gas_limit = chainspec.evm_config.block_gas_limit;

        let initial_cost = mtxn.initial_cost().value();

        let gas_limit = match &mtxn.gas_limit(chainspec) {
            Ok(gas_limit) => *gas_limit,
            Err(ite) => {
                return Err(ExecutionArtifactBuilderError::InvalidTransaction(
                    ite.clone(),
                ))
            }
        };
        let gas_limit_value = gas_limit.value();

        let baseline_motes_amount = chainspec.core_config.baseline_motes_amount_u512();
        let min_cost = match mtxn.min_cost(gas_limit_value, baseline_motes_amount) {
            Ok(motes) => motes.value(),
            Err(err) => return Err(ExecutionArtifactBuilderError::InvalidTransaction(err)),
        };

        let wei_per_mote = u128::from(chainspec.evm_config.wei_per_mote);
        let base_fee_wei = chainspec.evm_config.base_fee_wei();

        Ok(LimitsAndCosts::new(
            gas_limit,
            initial_cost,
            min_cost,
            wei_per_mote,
            base_fee_wei,
            evm_block_gas_limit,
        ))
    }
}

impl Default for LimitsAndCosts {
    fn default() -> Self {
        LimitsAndCosts {
            gas_limit: Gas::default(),
            initial_cost: U512::zero(),
            min_cost: U512::zero(),
            wei_per_mote: 0u128,
            base_fee_wei: 0u128,
            current_price: 0,
            consumed: None,
            refund: None,
            available: None,
            evm_block_gas_limit: 0u64,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExecutionArtifactBuilder {
    effects: Effects,
    hash: TransactionHash,
    header: TransactionHeader,
    error_message: Option<String>,
    messages: Messages,
    transfers: Vec<Transfer>,
    initiator: InitiatorAddr,

    limits_and_costs: LimitsAndCosts,
    // current_price: u8,
    // gas_limit: Gas,
    // initial_cost: U512,
    // min_cost: U512,
    //consumed: Gas,
    //refund: U512,
    // available: Option<U512>,
    size_estimate: u64,
    meta_transaction: MetaTransaction,

    is_penalized: Option<bool>,
    is_sufficient_balance: Option<bool>,

    evm_origin_resolution: Option<EvmOriginResolution>,
    evm_receipt: Option<EvmReceipt>,
}

impl ExecutionArtifactBuilder {
    pub(crate) fn try_new(
        txn: &Transaction,
        chainspec: &Chainspec,
        gas_price: u8,
    ) -> Result<Self, ExecutionArtifactBuilderError> {
        let meta_transaction =
            match MetaTransaction::new_from_txn_with_price(txn, chainspec, gas_price) {
                Ok(meta_transaction) => meta_transaction,
                Err(err) => return Err(ExecutionArtifactBuilderError::InvalidTransaction(err)),
            };

        let limits_and_costs = LimitsAndCosts::try_from_meta_txn(&meta_transaction, chainspec)?;

        Ok(ExecutionArtifactBuilder {
            effects: Effects::new(),
            hash: txn.hash(),
            header: txn.into(),
            initiator: meta_transaction.initiator_addr(),
            size_estimate: meta_transaction.size_estimate() as u64,
            meta_transaction,

            limits_and_costs,

            error_message: None,
            transfers: vec![],
            messages: Default::default(),
            is_penalized: None,
            is_sufficient_balance: None,
            evm_receipt: None,
            evm_origin_resolution: None,
        })
    }

    pub fn allow_execution(&self) -> bool {
        if self.error_message.is_some() {
            return false;
        }
        if let Some(penalized) = self.is_penalized {
            if penalized {
                return false;
            }
        }
        if let Some(is_sufficient_balance) = self.is_sufficient_balance {
            if is_sufficient_balance == false {
                return false;
            }
        }

        true
    }

    pub fn hash(&self) -> TransactionHash {
        self.hash
    }

    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator
    }

    pub fn balance_identifier_resolution(
        &self,
    ) -> Result<BalanceIdentifierResolution, ExecutionArtifactBuilderError> {
        let initiator_addr = self.initiator_addr().clone();
        let default_ret = match BalanceIdentifier::try_from(initiator_addr.clone()) {
            Ok(bi) => bi,
            Err(_) => {
                return Err(ExecutionArtifactBuilderError::BalanceIdentifierError(
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
                        return Err(ExecutionArtifactBuilderError::BalanceIdentifierError(
                            Box::new(BalanceIdentifierError::TryFromInitiatorAddr(initiator_addr)),
                        ))
                    }
                };

                Ok(BalanceIdentifierResolution::CheckEvmAccount(evm_ret))
            }
        }
    }

    pub fn authorization_keys(&self) -> BTreeSet<AccountHash> {
        self.meta_transaction.authorization_keys()
    }

    pub fn error_message(&self) -> Option<String> {
        self.error_message.clone()
    }

    pub fn gas_limit(&self) -> Gas {
        self.limits_and_costs.gas_limit()
    }

    pub fn limit(&self) -> U512 {
        self.limits_and_costs.gas_limit().value()
    }

    pub fn consumed(&self) -> U512 {
        self.limits_and_costs.consumed.unwrap_or_default().value()
    }

    pub fn available(&self) -> Option<U512> {
        self.limits_and_costs.available
    }

    pub fn cost_estimate(&self) -> Option<U512> {
        self.meta_transaction.cost_estimate()
    }

    pub fn cost_to_use(&self) -> U512 {
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

        match self.limits_and_costs.available {
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

    pub(crate) fn refund_amounts(&self) -> (U512, U512, u8) {
        let is_evm = self.meta_transaction.is_evm();
        let cost_to_use = self.cost_to_use();
        if is_evm {
            (cost_to_use, cost_to_use, 1)
        } else {
            (
                self.limit(),
                cost_to_use,
                self.limits_and_costs.current_price,
            )
        }
    }

    pub(crate) fn transaction_lane(&self) -> u8 {
        self.meta_transaction.transaction_lane()
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
                transaction_info: txn.to_transaction_info(),
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

    // *************** EVM **************

    pub fn evm_effective_gas_price(&self) -> Option<u128> {
        self.meta_transaction
            .evm_effective_gas_cost(self.limits_and_costs.base_fee_wei())
    }

    pub(crate) fn contract_direct_address(&self) -> Option<(HashAddr, String)> {
        self.meta_transaction.contract_direct_address()
    }
    pub fn evm_address(&self) -> Option<evm::Address> {
        if let Some(addr) = &self.initiator.evm_address() {
            return Some(*addr);
        }
        None
    }

    fn evm_addr_receipt(&self) -> Option<(evm::Address, EvmReceipt)> {
        if let Some(addr) = self.evm_address() {
            if let Some(receipt) = &self.evm_receipt {
                return Some((addr, receipt.clone()));
            }
        }
        None
    }

    pub fn evm_signer(&self) -> Option<Result<&PublicKey, EvmTransactionError>> {
        self.meta_transaction.evm_signer()
    }

    // *************** APPENDERS (aka "WITHS") **************

    pub fn with_gas_limit_consumed(&mut self) -> &mut Self {
        self.limits_and_costs.with_gas_limit_consumed();
        self
    }

    pub fn with_added_consumed(&mut self, consumed: Gas) -> &mut Self {
        self.limits_and_costs.with_added_consumed(consumed);
        self
    }

    pub fn with_appended_transfers(&mut self, transfers: &mut Vec<Transfer>) -> &mut Self {
        self.transfers.append(transfers);
        self
    }

    pub fn with_appended_effects(&mut self, effects: Effects) -> &mut Self {
        self.effects.append(effects);
        self
    }

    pub fn with_appended_messages(&mut self, messages: &mut Messages) -> &mut Self {
        self.messages.append(messages);
        self
    }

    pub fn with_state_result_error(&mut self, error: StateResultError) -> Result<&mut Self, ()> {
        if let StateResultError::RootNotFound = error {
            return Err(());
        }
        if self.error_message.is_none() {
            self.error_message = Some(format!("{:?}", error));
        }
        Ok(self)
    }

    pub fn with_evm_error(&mut self, error: EvmTransactionError) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{:?}", error));
        }
        self
    }

    pub fn with_initial_balance_result(&mut self, balance_result: &BalanceResult) -> &mut Self {
        // there is no point recording BalanceResult::RootNotFound because it is unrecoverable
        if let (None, Some(err)) = (&self.error_message, balance_result.error()) {
            self.error_message = Some(format!("{}", err));
        }
        if let Some(purse) = balance_result.purse_addr() {
            let minimum_amount = self.limits_and_costs.min_cost();
            let is_sufficient = balance_result.is_sufficient(minimum_amount);
            if !is_sufficient {
                self.error_message = Some(format!(
                    "Purse {} has less than {}",
                    base16::encode_lower(&purse),
                    minimum_amount
                ));
            }
        }
        let available = balance_result.available_balance().copied();
        self.limits_and_costs.with_available(available);
        self
    }

    pub fn with_wasm_v1_result(&mut self, wasm_v1_result: WasmV1Result) -> Result<&mut Self, ()> {
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

    pub fn with_error_message(&mut self, error_message: String) -> &mut Self {
        self.error_message = Some(error_message);
        self
    }

    pub fn with_handle_refund_result(
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

    pub fn with_handle_fee_result(
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

    pub fn with_balance_hold_result(
        &mut self,
        hold_result: &BalanceHoldResult,
    ) -> Result<&mut Self, ()> {
        if let BalanceHoldResult::RootNotFound = hold_result {
            return Err(());
        }
        if let BalanceHoldResult::Success { effects, .. } = hold_result {
            self.with_appended_effects(*effects.clone());
        }
        if let (None, BalanceHoldResult::Failure(_)) = (&self.error_message, hold_result) {
            self.error_message = hold_result.error_message();
            return Ok(self);
        }
        Ok(self)
    }

    pub fn with_refund_amount(&mut self, refund: U512) -> &mut Self {
        self.limits_and_costs.with_refund(refund);
        self
    }

    pub fn with_zero_cost(&mut self) -> &mut Self {
        self.limits_and_costs.with_zero_cost();
        self
    }

    pub fn with_evm_execution_outcome(
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

    pub fn with_invalid_wasm_v1_request(
        &mut self,
        invalid_request: &InvalidWasmV1Request,
    ) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{}", invalid_request));
        }
        self
    }

    pub fn with_auction_method_error(
        &mut self,
        auction_method_error: &AuctionMethodError,
    ) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{}", auction_method_error));
        }
        self
    }

    pub fn with_transfer_result(
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

    pub fn with_burn_result(&mut self, burn_result: BurnResult) -> Result<&mut Self, ()> {
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

    pub fn with_bidding_result(&mut self, bidding_result: BiddingResult) -> Result<&mut Self, ()> {
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

    pub fn with_is_penalized(&mut self, penalized: bool) -> &mut Self {
        self.is_penalized = Some(penalized);
        self
    }

    pub fn with_is_sufficient_balance(&mut self, sufficient_balance: bool) -> &mut Self {
        self.is_sufficient_balance = Some(sufficient_balance);
        self
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

    pub fn with_exec_attempt(&mut self) -> &mut Self {
        let is_penalized = match self.is_penalized {
            Some(true) => true,
            Some(false) | None => return self,
        };
        let is_insufficient_balance = match self.is_sufficient_balance {
            Some(false) => true,
            Some(true) | None => return self,
        };

        let err_msg = format!(
            "exec attempt while penalized: {} or insufficient balance: {}",
            is_penalized, is_insufficient_balance
        );

        if self.error_message().is_none() {
            self.with_error_message(err_msg);
        }
        self
    }

    pub fn with_evm_origin_resolution(
        &mut self,
        resolution: Option<EvmOriginResolution>,
    ) -> &mut Self {
        self.evm_origin_resolution = resolution;
        self
    }

    // *************** BUILD ****************
    // NOTE: by convention, build should always be the final function in the impl.
    pub(crate) fn build(self) -> ExecutionArtifact {
        let actual_cost = self.cost_to_use();

        let execution_result = if let Some((initiator, receipt)) = &self.evm_addr_receipt() {
            let result = EvmExecutionResult {
                initiator: *initiator,
                current_price: self.limits_and_costs.current_price(),
                limit: self.limits_and_costs.gas_limit(),
                cost: actual_cost,
                refund: self.limits_and_costs.refund(),
                size_estimate: self.size_estimate,
                effects: self.effects,
                receipt: receipt.clone(),
            };
            ExecutionResult::from(result)
        } else {
            let result = ExecutionResultV2 {
                effects: self.effects,
                transfers: self.transfers,
                initiator: self.initiator,
                refund: self.limits_and_costs.refund(),
                limit: self.limits_and_costs.gas_limit(),
                consumed: self.limits_and_costs.consumed(),
                cost: actual_cost,
                current_price: self.limits_and_costs.current_price(),
                size_estimate: self.size_estimate,
                error_message: self.error_message,
            };
            ExecutionResult::V2(Box::new(result))
        };
        ExecutionArtifact::new(
            self.hash,
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
