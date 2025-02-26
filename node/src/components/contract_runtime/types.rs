use std::{collections::BTreeMap, sync::Arc};

use crate::{contract_runtime::StateResultError, types::TransactionHeader};
use casper_types::{InitiatorAddr, Transfer};
use datasize::DataSize;
use serde::Serialize;

use casper_execution_engine::engine_state::{
    Error, InvalidRequest as InvalidWasmV1Request, WasmV1Result,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        auction::AuctionMethodError, BalanceHoldResult, BalanceResult, BiddingResult,
        EraValidatorsRequest, HandleFeeResult, HandleRefundResult, TransferResult,
    },
};
use casper_types::{
    contract_messages::Messages,
    execution::{Effects, ExecutionResult, ExecutionResultV2},
    BlockHash, BlockHeaderV2, BlockV2, Digest, EraId, Gas, InvalidDeploy, InvalidTransaction,
    InvalidTransactionV1, ProtocolVersion, PublicKey, Transaction, TransactionHash, U512,
};

use self::wasm_v2_request::{WasmV2Error, WasmV2Result};

use super::operations::wasm_v2_request;

/// Request for validator weights for a specific era.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorWeightsByEraIdRequest {
    state_hash: Digest,
    era_id: EraId,
    protocol_version: ProtocolVersion,
}

impl ValidatorWeightsByEraIdRequest {
    /// Constructs a new ValidatorWeightsByEraIdRequest.
    pub fn new(state_hash: Digest, era_id: EraId, protocol_version: ProtocolVersion) -> Self {
        ValidatorWeightsByEraIdRequest {
            state_hash,
            era_id,
            protocol_version,
        }
    }

    /// Get the state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Get the era id.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Get the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

impl From<ValidatorWeightsByEraIdRequest> for EraValidatorsRequest {
    fn from(input: ValidatorWeightsByEraIdRequest) -> Self {
        EraValidatorsRequest::new(input.state_hash)
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
    cost: U512,
    limit: Gas,
    consumed: Gas,
    size_estimate: u64,
    baseline_amount: U512,
}

impl ExecutionArtifactBuilder {
    pub fn new(transaction: &Transaction, baseline_amount: U512) -> Self {
        ExecutionArtifactBuilder {
            effects: Effects::new(),
            hash: transaction.hash(),
            header: transaction.into(),
            error_message: None,
            transfers: vec![],
            messages: Default::default(),
            initiator: transaction.initiator_addr(),
            cost: U512::zero(),
            limit: Gas::zero(),
            consumed: Gas::zero(),
            size_estimate: transaction.size_estimate() as u64,
            baseline_amount,
        }
    }

    pub fn error_message(&self) -> Option<String> {
        self.error_message.clone()
    }

    pub fn consumed(&self) -> U512 {
        // to prevent do-nothing exhaustion, if consumed == 0 we raise consumed to baseline
        let consumed = self.consumed;
        if consumed == Gas::zero() {
            self.baseline_amount
        } else {
            consumed.value()
        }
    }

    pub fn with_added_consumed(&mut self, consumed: Gas) -> &mut Self {
        self.consumed = self.consumed.saturating_add(consumed);
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

    pub fn with_initial_balance_result(
        &mut self,
        balance_result: BalanceResult,
        minimum_amount: U512,
    ) -> Result<&mut Self, bool> {
        if let BalanceResult::RootNotFound = balance_result {
            return Err(true);
        }
        if let (None, Some(err)) = (&self.error_message, balance_result.error()) {
            self.error_message = Some(format!("{}", err));
            return Err(false);
        }
        if let Some(purse) = balance_result.purse_addr() {
            let is_sufficient = balance_result.is_sufficient(minimum_amount);
            if !is_sufficient {
                self.error_message = Some(format!(
                    "Purse {} has less than {}",
                    base16::encode_lower(&purse),
                    minimum_amount
                ));
                return Ok(self);
            }
        }
        Ok(self)
    }

    pub fn with_wasm_v1_result(&mut self, wasm_v1_result: WasmV1Result) -> Result<&mut Self, ()> {
        if let Some(Error::RootNotFound(_)) = wasm_v1_result.error() {
            return Err(());
        }
        self.with_added_consumed(wasm_v1_result.consumed());
        if let (None, Some(err)) = (&self.error_message, wasm_v1_result.error()) {
            self.error_message = Some(format!("{}", err));
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

    pub fn with_set_refund_purse_result(
        &mut self,
        handle_refund_result: &HandleRefundResult,
    ) -> Result<&mut Self, bool> {
        if let HandleRefundResult::RootNotFound = handle_refund_result {
            return Err(true);
        }
        self.with_appended_effects(handle_refund_result.effects());
        if let (None, HandleRefundResult::Failure(_)) = (&self.error_message, handle_refund_result)
        {
            self.error_message = handle_refund_result.error_message();
            return Err(false);
        }
        Ok(self)
    }

    pub fn with_clear_refund_purse_result(
        &mut self,
        handle_refund_result: &HandleRefundResult,
    ) -> Result<&mut Self, bool> {
        if let HandleRefundResult::RootNotFound = handle_refund_result {
            return Err(true);
        }
        self.with_appended_effects(handle_refund_result.effects());
        if let (None, HandleRefundResult::Failure(_)) = (&self.error_message, handle_refund_result)
        {
            self.error_message = handle_refund_result.error_message();
            return Err(false);
        }
        Ok(self)
    }

    pub fn with_handle_refund_result(
        &mut self,
        handle_refund_result: &HandleRefundResult,
    ) -> Result<&mut Self, ()> {
        if let HandleRefundResult::RootNotFound = handle_refund_result {
            return Err(());
        }
        if let (None, HandleRefundResult::Failure(_)) = (&self.error_message, handle_refund_result)
        {
            self.error_message = handle_refund_result.error_message();
            return Ok(self);
        }
        self.with_appended_effects(handle_refund_result.effects());
        Ok(self)
    }

    pub fn with_handle_fee_result(
        &mut self,
        handle_fee_result: &HandleFeeResult,
    ) -> Result<&mut Self, ()> {
        if let HandleFeeResult::RootNotFound = handle_fee_result {
            return Err(());
        }
        if let (None, HandleFeeResult::Failure(err)) = (&self.error_message, handle_fee_result) {
            self.error_message = Some(format!("{}", err));
            return Ok(self);
        }
        self.with_appended_effects(handle_fee_result.effects());
        Ok(self)
    }

    pub fn with_balance_hold_result(
        &mut self,
        hold_result: &BalanceHoldResult,
    ) -> Result<&mut Self, ()> {
        if let BalanceHoldResult::RootNotFound = hold_result {
            return Err(());
        }
        if let (None, BalanceHoldResult::Failure(err)) = (&self.error_message, hold_result) {
            self.error_message = Some(format!("{}", err));
            return Ok(self);
        }
        self.with_appended_effects(hold_result.effects());
        Ok(self)
    }

    pub fn with_added_cost(&mut self, cost: U512) -> &mut Self {
        self.cost = self.cost.saturating_add(cost);
        self
    }

    pub fn with_gas_limit(&mut self, limit: Gas) -> &mut Self {
        self.limit = limit;
        self
    }

    pub fn with_invalid_transaction(
        &mut self,
        invalid_transaction: &InvalidTransaction,
    ) -> &mut Self {
        if self.error_message.is_none() {
            self.error_message = Some(format!("{}", invalid_transaction));
        }
        self
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
            mut transfers,
            effects,
            cache: _,
        } = transfer_result
        {
            self.with_appended_transfers(&mut transfers)
                .with_appended_effects(effects);
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
        if let BiddingResult::Success { effects, .. } = bidding_result {
            self.with_appended_effects(effects);
        }
        Ok(self)
    }

    #[allow(unused)]
    pub fn with_initiator_addr(&mut self, initiator_addr: InitiatorAddr) -> &mut Self {
        self.initiator = initiator_addr;
        self
    }

    pub(crate) fn build(self) -> ExecutionArtifact {
        let consumed = Gas::new(self.consumed());
        let result = ExecutionResultV2 {
            effects: self.effects,
            transfers: self.transfers,
            initiator: self.initiator,
            limit: self.limit,
            consumed,
            cost: self.cost,
            size_estimate: self.size_estimate,
            error_message: self.error_message,
        };
        let execution_result = ExecutionResult::V2(Box::new(result));
        ExecutionArtifact::new(self.hash, self.header, execution_result, self.messages)
    }

    /// Adds the error message from a `InvalidRequest` to the artifact.
    pub(crate) fn with_invalid_wasm_v2_request(
        &mut self,
        ire: wasm_v2_request::InvalidRequest,
    ) -> &mut Self {
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
}

/// Effects from running step and the next era validators that are gathered when an era ends.
#[derive(Clone, Debug, DataSize)]
pub(crate) struct StepOutcome {
    /// Validator sets for all upcoming eras that have already been determined.
    pub(crate) upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    /// An [`Effects`] created by an era ending.
    pub(crate) step_effects: Effects,
}

#[derive(Clone, Debug, DataSize, PartialEq, Eq, Serialize)]
pub(crate) struct ExecutionArtifact {
    pub(crate) transaction_hash: TransactionHash,
    pub(crate) transaction_header: TransactionHeader,
    pub(crate) execution_result: ExecutionResult,
    pub(crate) messages: Messages,
}

impl ExecutionArtifact {
    pub(crate) fn new(
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        execution_result: ExecutionResult,
        messages: Messages,
    ) -> Self {
        Self {
            transaction_hash,
            transaction_header,
            execution_result,
            messages,
        }
    }
}

#[doc(hidden)]
/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Clone, Debug, DataSize)]
pub struct BlockAndExecutionArtifacts {
    /// The [`Block`] the contract runtime executed.
    pub(crate) block: Arc<BlockV2>,
    /// The [`ApprovalsHashes`] for the deploys in this block.
    pub(crate) approvals_hashes: Box<ApprovalsHashes>,
    /// The results from executing the transactions in the block.
    pub(crate) execution_artifacts: Vec<ExecutionArtifact>,
    /// The [`Effects`] and the upcoming validator sets determined by the `step`
    pub(crate) step_outcome: Option<StepOutcome>,
}

/// Type representing results of the speculative execution.
#[derive(Debug)]
pub enum SpeculativeExecutionResult {
    InvalidTransaction(InvalidTransaction),
    WasmV1(Box<casper_binary_port::SpeculativeExecutionResult>),
    ReceivedV1Transaction,
}

impl SpeculativeExecutionResult {
    pub fn invalid_gas_limit(transaction: Transaction) -> Self {
        match transaction {
            Transaction::Deploy(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::Deploy(InvalidDeploy::UnableToCalculateGasLimit),
            ),
            Transaction::V1(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::V1(InvalidTransactionV1::UnableToCalculateGasLimit),
            ),
        }
    }

    pub fn invalid_transaction(error: InvalidTransaction) -> Self {
        SpeculativeExecutionResult::InvalidTransaction(error)
    }
}

/// State to use to construct the next block in the blockchain. Includes the state root hash for the
/// execution engine as well as certain values the next header will be based on.
#[derive(DataSize, Default, Debug, Clone, Serialize)]
pub struct ExecutionPreState {
    /// The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    next_block_height: u64,
    /// The state root to use when executing deploys.
    pre_state_root_hash: Digest,
    /// The parent hash of the next `Block`.
    parent_hash: BlockHash,
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    parent_seed: Digest,
}

impl ExecutionPreState {
    pub(crate) fn new(
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) -> Self {
        ExecutionPreState {
            next_block_height,
            pre_state_root_hash,
            parent_hash,
            parent_seed,
        }
    }

    /// Creates instance of `ExecutionPreState` from given block header nad Merkle tree hash
    /// activation point.
    pub fn from_block_header(block_header: &BlockHeaderV2) -> Self {
        ExecutionPreState {
            pre_state_root_hash: *block_header.state_root_hash(),
            next_block_height: block_header.height() + 1,
            parent_hash: block_header.block_hash(),
            parent_seed: *block_header.accumulated_seed(),
        }
    }

    // The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    pub fn next_block_height(&self) -> u64 {
        self.next_block_height
    }
    /// The state root to use when executing deploys.
    pub fn pre_state_root_hash(&self) -> Digest {
        self.pre_state_root_hash
    }
    /// The parent hash of the next `Block`.
    pub fn parent_hash(&self) -> BlockHash {
        self.parent_hash
    }
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    pub fn parent_seed(&self) -> Digest {
        self.parent_seed
    }
}

#[derive(Clone, Copy, Ord, Eq, PartialOrd, PartialEq, DataSize, Debug)]
pub(crate) struct EraPrice {
    era_id: EraId,
    gas_price: u8,
}

impl EraPrice {
    pub(crate) fn new(era_id: EraId, gas_price: u8) -> Self {
        Self { era_id, gas_price }
    }

    pub(crate) fn gas_price(&self) -> u8 {
        self.gas_price
    }

    pub(crate) fn maybe_gas_price_for_era_id(&self, era_id: EraId) -> Option<u8> {
        if self.era_id == era_id {
            return Some(self.gas_price);
        }

        None
    }
}
