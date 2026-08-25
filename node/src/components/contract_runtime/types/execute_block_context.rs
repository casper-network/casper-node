use crate::{
    contract_runtime::{
        types::TransactionProcessContext, BlockAndExecutionArtifacts, BlockExecutionError,
        ExecutionArtifact, ExecutionPreState, StepOutcome,
    },
    types::{
        transaction::{WasmV2InvalidRequest, WasmV2Request},
        ExecutableBlock, InternalEraReport,
    },
};
use casper_execution_engine::engine_state::{
    BlockInfo, InvalidRequest, SessionInputData, WasmV1Request,
};
use casper_executor_evm::{
    BlockContext as EvmV1BlockContext, ExecuteKind as EvmExecuteKind,
    ExecuteRequest as EvmV1ExecuteRequest,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        balance::BalanceHandling, mint::BurnRequest, AuctionMethod, BalanceHoldKind,
        BalanceHoldRequest, BalanceIdentifier, BalanceRequest, BiddingRequest, BlockGlobalRequest,
        BlockRewardsRequest, EraValidatorsRequest, EvictItem, FeeRequest, HandleFeeMode,
        HandleFeeRequest, HandleRefundMode, HandleRefundRequest, InsufficientBalanceHandling,
        ProofHandling, PruneRequest, StepRequest, TransferRequest,
    },
    system::runtime_native::Config as NativeRuntimeConfig,
};
use casper_types::{
    bytesrepr, bytesrepr::ToBytes, evm::Address as EvmAddress, global_state::TrieMerkleProof,
    ApprovalsHash, BlockHash, BlockTime, BlockV2, Chainspec, Digest, EraEndV2, EraId,
    EvmTransaction, FeeHandling, Key, ProtocolVersion, PublicKey, RefundHandling, RuntimeArgs,
    StoredValue, Transaction, U512,
};
use itertools::Itertools;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc, time::Instant};
use thiserror::Error;
use tracing::error;

pub(crate) enum ExecuteBlockOutcome {
    FailedToCreateEraEnd {
        elapsed: f64,
        err_msg: String,
        /// An optional `EraReport` we tried to use to construct an `EraEnd`.
        maybe_era_report: Option<InternalEraReport>,
        /// An optional map of the next era validator weights used to construct an `EraEnd`.
        maybe_next_era_validator_weights: Option<(BTreeMap<PublicKey, U512>, u8)>,
    },
    Success {
        elapsed: f64,
        ret: BlockAndExecutionArtifacts,
    },
}

impl ExecuteBlockOutcome {
    pub(crate) fn total_elapsed(&self) -> f64 {
        match self {
            ExecuteBlockOutcome::FailedToCreateEraEnd { elapsed, .. } => *elapsed,
            ExecuteBlockOutcome::Success { elapsed, .. } => *elapsed,
        }
    }
}

/// An error during block execution.
#[derive(Debug, Error, Serialize)]
pub(crate) enum ExecuteBlockContextError {
    /// Both the block to be executed and the execution pre-state specify the height of the next
    /// block. These must agree and this error will be thrown if they do not.
    #[error(
        "block's height does not agree with execution pre-state. \
         block: {executable_block:?}, \
         execution pre-state: {execution_pre_state:?}"
    )]
    WrongBlockHeight {
        /// The finalized block the system attempted to execute.
        executable_block: Box<ExecutableBlock>,
        /// The state of the blockchain prior to block execution that was to be used.
        execution_pre_state: Box<ExecutionPreState>,
    },
    #[error("Failed to get new era gas price when executing switch block")]
    FailedToGetNewEraGasPrice { era_id: EraId },
    #[error("Chainspec addressable entity setting misaligned with block setting")]
    InvalidAESetting(bool),

    #[error("Unable to generate transaction ids checksum")]
    FailedToComputeApprovalsChecksum(bytesrepr::Error),
}

pub(crate) struct HandlingSettings {
    network_name: String,
    insufficient_balance_handling: InsufficientBalanceHandling,
    balance_handling: BalanceHandling,
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
}

impl HandlingSettings {
    pub(crate) fn new(
        network_name: String,
        insufficient_balance_handling: InsufficientBalanceHandling,
        balance_handling: BalanceHandling,
        refund_handling: RefundHandling,
        fee_handling: FeeHandling,
    ) -> Self {
        HandlingSettings {
            network_name,
            insufficient_balance_handling,
            balance_handling,
            refund_handling,
            fee_handling,
        }
    }

    pub(crate) fn network_name(&self) -> String {
        self.network_name.clone()
    }

    pub(crate) fn insufficient_balance_handling(&self) -> InsufficientBalanceHandling {
        self.insufficient_balance_handling
    }

    pub(crate) fn balance_handling(&self) -> BalanceHandling {
        self.balance_handling
    }

    pub(crate) fn refund_handling(&self) -> RefundHandling {
        self.refund_handling
    }

    pub(crate) fn fee_handling(&self) -> FeeHandling {
        self.fee_handling
    }
}

pub(crate) struct ExecuteBlockTiming {
    pre_process: Instant,
    process: Option<Instant>,
    post_process: Option<Instant>,
    wasm_v1: Option<Instant>,
    wasm_v2: Option<Instant>,
    evm_v1: Option<Instant>,
    block_rewards: Option<Instant>,
    step: Option<Instant>,
    prune: Option<Instant>,
    db_write: Option<Instant>,
    db_flush: Option<Instant>,
}

impl ExecuteBlockTiming {
    pub(crate) fn new(pre_process: Instant) -> Self {
        Self {
            pre_process,
            process: None,
            post_process: None,
            wasm_v1: None,
            wasm_v2: None,
            evm_v1: None,
            block_rewards: None,
            step: None,
            prune: None,
            db_write: None,
            db_flush: None,
        }
    }

    pub(crate) fn pre_process(&self) -> Instant {
        self.pre_process
    }

    pub(crate) fn process(&self) -> Option<Instant> {
        self.process
    }

    pub(crate) fn post_process(&self) -> Option<Instant> {
        self.post_process
    }

    pub(crate) fn wasm_v1(&self) -> Option<Instant> {
        self.wasm_v1
    }

    pub(crate) fn wasm_v2(&self) -> Option<Instant> {
        self.wasm_v2
    }

    pub(crate) fn evm_v1(&self) -> Option<Instant> {
        self.evm_v1
    }

    pub(crate) fn block_rewards(&self) -> Option<Instant> {
        self.block_rewards
    }

    pub(crate) fn step(&self) -> Option<Instant> {
        self.step
    }

    pub(crate) fn prune(&self) -> Option<Instant> {
        self.prune
    }

    pub(crate) fn db_write(&self) -> Option<Instant> {
        self.db_write
    }

    pub(crate) fn db_flush(&self) -> Option<Instant> {
        self.db_flush
    }

    pub(crate) fn with_process(&mut self, process: Instant) -> &mut Self {
        self.process = Some(process);
        self
    }

    pub(crate) fn with_post_process(&mut self, post_process: Instant) -> &mut Self {
        self.post_process = Some(post_process);
        self
    }

    pub(crate) fn with_wasm_v1_starting(&mut self, wasm_v1_starting: Instant) -> &mut Self {
        self.wasm_v1 = Some(wasm_v1_starting);
        self
    }

    pub(crate) fn with_wasm_v2_starting(&mut self, wasm_v2_starting: Instant) -> &mut Self {
        self.wasm_v2 = Some(wasm_v2_starting);
        self
    }

    pub(crate) fn with_evm_v1_starting(&mut self, evm_v1_starting: Instant) -> &mut Self {
        self.evm_v1 = Some(evm_v1_starting);
        self
    }

    pub(crate) fn with_block_rewards_starting(
        &mut self,
        block_rewards_starting: Instant,
    ) -> &mut Self {
        self.block_rewards = Some(block_rewards_starting);
        self
    }

    pub(crate) fn with_step_starting(&mut self, step_starting: Instant) -> &mut Self {
        self.step = Some(step_starting);
        self
    }

    pub(crate) fn with_prune_starting(&mut self, prune_starting: Instant) -> &mut Self {
        self.prune = Some(prune_starting);
        self
    }

    pub(crate) fn with_db_write_starting(&mut self, db_write_starting: Instant) -> &mut Self {
        self.db_write = Some(db_write_starting);
        self
    }

    pub(crate) fn with_db_flush_starting(&mut self, db_flush_starting: Instant) -> &mut Self {
        self.db_flush = Some(db_flush_starting);
        self
    }
}

pub(crate) struct ExecuteBlockContext {
    pre_state: ExecutionPreState,
    executable_block: ExecutableBlock,
    last_switch_block_hash: Option<BlockHash>,
    current_gas_price: u8,
    next_era_gas_price: Option<u8>,

    handling_settings: HandlingSettings,
    transaction_ids_checksum: Digest,
    approval_hashes: Vec<ApprovalsHash>,
    native_runtime_config: NativeRuntimeConfig,
    protocol_version: ProtocolVersion,
    activation_point_era_id: EraId,
    prune_batch_size: u64,
    addressable_entity_enabled: bool,

    // mutable variables below this line
    timing: ExecuteBlockTiming,
    state_root_hash: Digest,
    execution_artifacts: Vec<ExecutionArtifact>,
    step_outcome: Option<StepOutcome>,
}

impl ExecuteBlockContext {
    pub(crate) fn try_new(
        executable_block: &ExecutableBlock,
        execution_pre_state: &ExecutionPreState,
        chainspec: &Chainspec,
        current_gas_price: u8,
        next_era_gas_price: Option<u8>,
        last_switch_block_hash: Option<BlockHash>,
        addressable_entity_enabled: bool,
    ) -> Result<Self, ExecuteBlockContextError> {
        let timing = ExecuteBlockTiming::new(Instant::now());
        let enable_addressable_entity = chainspec.core_config.enable_addressable_entity();
        if enable_addressable_entity != addressable_entity_enabled {
            return Err(ExecuteBlockContextError::InvalidAESetting(
                addressable_entity_enabled,
            ));
        }
        let block_height = executable_block.height;
        if block_height != execution_pre_state.next_block_height() {
            return Err(ExecuteBlockContextError::WrongBlockHeight {
                executable_block: Box::new(executable_block.clone()),
                execution_pre_state: Box::new(execution_pre_state.clone()),
            });
        }
        if executable_block.era_report.is_some() && next_era_gas_price.is_none() {
            return Err(ExecuteBlockContextError::FailedToGetNewEraGasPrice {
                era_id: executable_block.era_id.successor(),
            });
        }

        let transaction_ids = executable_block
            .transactions
            .iter()
            .map(Transaction::compute_id)
            .collect_vec();

        let approval_hashes = transaction_ids
            .clone()
            .into_iter()
            .map(|id| id.approvals_hash())
            .collect();

        let transaction_ids_checksum = match transaction_ids.into_bytes() {
            Ok(bytes) => Digest::hash(bytes),
            Err(err) => {
                return Err(ExecuteBlockContextError::FailedToComputeApprovalsChecksum(
                    err,
                ))
            }
        };

        let protocol_version = chainspec.protocol_version();
        let activation_point_era_id = chainspec.protocol_config.activation_point.era_id();
        let prune_batch_size = chainspec.core_config.prune_batch_size;
        let addressable_entity_enabled = chainspec.core_config.enable_addressable_entity();

        // set up network handling settings
        let network_name = chainspec.network_config.name.clone();
        let insufficient_balance_handling = InsufficientBalanceHandling::HoldRemaining;
        let balance_handling = BalanceHandling::Available;
        let refund_handling = chainspec.core_config.refund_handling;
        let fee_handling = chainspec.core_config.fee_handling;
        let handling_settings = HandlingSettings::new(
            network_name,
            insufficient_balance_handling,
            balance_handling,
            refund_handling,
            fee_handling,
        );

        let state_root_hash = execution_pre_state.pre_state_root_hash();

        let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);

        let artifacts = Vec::with_capacity(executable_block.transactions.len());

        Ok(ExecuteBlockContext {
            pre_state: execution_pre_state.clone(),
            executable_block: executable_block.clone(),
            current_gas_price,
            next_era_gas_price,
            last_switch_block_hash,
            transaction_ids_checksum,
            approval_hashes,
            protocol_version,
            activation_point_era_id,
            prune_batch_size,
            addressable_entity_enabled,
            native_runtime_config,
            handling_settings,
            timing,
            state_root_hash,
            execution_artifacts: artifacts,
            step_outcome: None,
        })
    }

    pub(crate) fn transaction_ids_checksum(&self) -> Digest {
        self.transaction_ids_checksum
    }

    pub(crate) fn execution_artifacts(&self) -> Vec<ExecutionArtifact> {
        self.execution_artifacts.clone()
    }

    // pub(crate) fn execution_results_iter<'a>(
    //     &self,
    // ) -> impl Iterator<Item = &'a ExecutionResult> + Clone + use<'a, '_> {
    //     self.execution_artifacts
    //         .iter()
    //         .map(move |artifact| &artifact.execution_result)
    // }

    pub(crate) fn block_height(&self) -> u64 {
        self.executable_block.height
    }

    pub(crate) fn prev_block_height(&self) -> Option<u64> {
        self.executable_block.height.checked_sub(1)
    }

    pub(crate) fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub(crate) fn activation_point_era_id(&self) -> EraId {
        self.activation_point_era_id
    }

    pub(crate) fn prune_batch_size(&self) -> u64 {
        self.prune_batch_size
    }

    pub(crate) fn addressable_entity_enabled(&self) -> bool {
        self.addressable_entity_enabled
    }

    pub(crate) fn block_time(&self) -> BlockTime {
        BlockTime::new(self.executable_block.timestamp.millis())
    }

    pub(crate) fn block_time_millis(&self) -> u64 {
        self.executable_block.timestamp.millis()
    }

    pub(crate) fn proposer(&self) -> Box<PublicKey> {
        self.executable_block.proposer.clone()
    }

    pub(crate) fn era_id(&self) -> EraId {
        self.executable_block.era_id
    }

    pub(crate) fn state_root_hash(&self) -> Digest {
        self.state_root_hash
    }

    pub(crate) fn parent_block_hash(&self) -> BlockHash {
        self.pre_state.parent_hash()
    }

    pub(crate) fn network_name(&self) -> String {
        self.handling_settings.network_name()
    }

    pub(crate) fn refund_handling(&self) -> RefundHandling {
        self.handling_settings.refund_handling()
    }

    pub(crate) fn fee_handling(&self) -> FeeHandling {
        self.handling_settings.fee_handling()
    }

    pub(crate) fn pre_process_elapsed(&self) -> f64 {
        self.timing.pre_process().elapsed().as_secs_f64()
    }

    pub(crate) fn process_elapsed(&self) -> f64 {
        match self.timing.process() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn post_process_elapsed(&self) -> f64 {
        match self.timing.post_process() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn wasm_v1_elapsed(&self) -> f64 {
        match self.timing.wasm_v1() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn wasm_v2_elapsed(&self) -> f64 {
        match self.timing.wasm_v2() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn evm_v1_elapsed(&self) -> f64 {
        match self.timing.evm_v1() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn block_rewards_elapsed(&self) -> f64 {
        match self.timing.block_rewards() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn step_elapsed(&self) -> f64 {
        match self.timing.step() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn prune_elapsed(&self) -> f64 {
        match self.timing.prune() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn db_write_elapsed(&self) -> f64 {
        match self.timing.db_write() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn db_flush_elapsed(&self) -> f64 {
        match self.timing.db_flush() {
            Some(instant) => instant.elapsed().as_secs_f64(),
            None => f64::default(),
        }
    }

    pub(crate) fn process_starting(&mut self) -> &mut Self {
        self.timing.with_process(Instant::now());
        self
    }

    pub(crate) fn post_process_starting(&mut self) -> &mut Self {
        self.timing.with_post_process(Instant::now());
        self
    }

    pub(crate) fn wasm_v1_starting(&mut self) -> &mut Self {
        self.timing.with_wasm_v1_starting(Instant::now());
        self
    }

    pub(crate) fn wasm_v2_starting(&mut self) -> &mut Self {
        self.timing.with_wasm_v2_starting(Instant::now());
        self
    }

    pub(crate) fn evm_v1_starting(&mut self) -> &mut Self {
        self.timing.with_evm_v1_starting(Instant::now());
        self
    }

    pub(crate) fn block_rewards_starting(&mut self) -> &mut Self {
        self.timing.with_block_rewards_starting(Instant::now());
        self
    }

    pub(crate) fn step_starting(&mut self) -> &mut Self {
        self.timing.with_step_starting(Instant::now());
        self
    }

    pub(crate) fn prune_starting(&mut self) -> &mut Self {
        self.timing.with_prune_starting(Instant::now());
        self
    }

    pub(crate) fn db_write_starting(&mut self) -> &mut Self {
        self.timing.with_db_write_starting(Instant::now());
        self
    }

    pub(crate) fn db_flush_starting(&mut self) -> &mut Self {
        self.timing.with_db_flush_starting(Instant::now());
        self
    }

    pub(crate) fn with_state_root_hash(&mut self, state_root_hash: Digest) -> &mut Self {
        self.state_root_hash = state_root_hash;
        self
    }

    pub(crate) fn with_artifact(&mut self, artifact: ExecutionArtifact) -> &mut Self {
        self.execution_artifacts.push(artifact);
        self
    }

    pub(crate) fn with_step_outcome(&mut self, step_outcome: StepOutcome) -> &mut Self {
        self.step_outcome = Some(step_outcome);
        self
    }
}

impl ExecuteBlockContext {
    pub(crate) fn root_not_found(&self) -> BlockExecutionError {
        BlockExecutionError::RootNotFound(self.state_root_hash)
    }

    pub(crate) fn block_global_request(&self) -> BlockGlobalRequest {
        BlockGlobalRequest::set_block_info(
            self.state_root_hash,
            self.block_time(),
            self.protocol_version,
            self.addressable_entity_enabled,
        )
    }

    pub(crate) fn balance_request(&self, balance_identifier: BalanceIdentifier) -> BalanceRequest {
        BalanceRequest::new(
            self.state_root_hash,
            self.protocol_version,
            balance_identifier,
            self.handling_settings.balance_handling(),
            ProofHandling::NoProofs,
        )
    }

    pub(crate) fn balance_hold_request(
        &self,
        balance_identifier: BalanceIdentifier,
        hold_amount: U512,
    ) -> BalanceHoldRequest {
        BalanceHoldRequest::new_processing_hold(
            self.state_root_hash,
            self.protocol_version,
            balance_identifier,
            hold_amount,
            self.handling_settings.insufficient_balance_handling(),
        )
    }

    pub(crate) fn gas_hold_request(
        &self,
        balance_identifier: BalanceIdentifier,
        hold_amount: U512,
    ) -> BalanceHoldRequest {
        BalanceHoldRequest::new_gas_hold(
            self.state_root_hash,
            self.protocol_version,
            balance_identifier,
            hold_amount,
            self.handling_settings.insufficient_balance_handling(),
        )
    }

    pub(crate) fn clear_balance_hold_request(
        &self,
        hold_kind: BalanceHoldKind,
        balance_identifier: BalanceIdentifier,
    ) -> BalanceHoldRequest {
        BalanceHoldRequest::new_clear(
            self.state_root_hash,
            self.protocol_version,
            hold_kind,
            balance_identifier,
        )
    }

    pub(crate) fn transfer_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        runtime_args: RuntimeArgs,
    ) -> TransferRequest {
        TransferRequest::with_runtime_args(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            txn_ctx.transaction_hash(),
            txn_ctx.initiator_addr(),
            txn_ctx.authorization_keys(),
            runtime_args,
        )
    }

    pub(crate) fn burn_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        runtime_args: RuntimeArgs,
    ) -> BurnRequest {
        BurnRequest::with_runtime_args(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            txn_ctx.transaction_hash(),
            txn_ctx.initiator_addr(),
            txn_ctx.authorization_keys(),
            runtime_args,
        )
    }

    pub(crate) fn bidding_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        auction_method: AuctionMethod,
    ) -> BiddingRequest {
        BiddingRequest::new(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            txn_ctx.transaction_hash(),
            txn_ctx.initiator_addr(),
            txn_ctx.authorization_keys(),
            auction_method,
        )
    }

    pub(crate) fn wasm_v1_session_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        input: &SessionInputData,
    ) -> Result<WasmV1Request, InvalidRequest> {
        WasmV1Request::new_session(
            BlockInfo::new(
                self.state_root_hash,
                self.block_time(),
                self.parent_block_hash(),
                self.block_height(),
                self.protocol_version,
            ),
            txn_ctx.gas_limit(),
            input,
        )
    }

    pub(crate) fn wasm_v2_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        input: crate::types::transaction::WasmV2TransactionInput,
    ) -> Result<WasmV2Request, WasmV2InvalidRequest> {
        WasmV2Request::new(
            txn_ctx.gas_limit(),
            self.network_name(),
            self.state_root_hash,
            self.parent_block_hash(),
            self.block_height(),
            input,
        )
    }

    pub(crate) fn evm_v1_request(
        &self,
        evm_txn: EvmTransaction,
        block_gas_limit: u64,
        base_fee_wei: u128,
    ) -> EvmV1ExecuteRequest {
        let block_context = EvmV1BlockContext::new(
            self.block_height(),
            self.block_time(),
            block_gas_limit,
            base_fee_wei,
            EvmAddress::from_block_proposer_public_key(&self.proposer()),
        );
        EvmV1ExecuteRequest {
            block: block_context,
            kind: EvmExecuteKind::Transaction(Box::new(evm_txn)),
        }
    }

    pub(crate) fn refund_mode(
        &self,
        txn_ctx: &TransactionProcessContext,
    ) -> Option<HandleRefundMode> {
        let balance_identifier = match txn_ctx.balance_identifier() {
            Some(balance_identifier) => balance_identifier.clone(),
            None => return None,
        };

        let limit = txn_ctx.gas_limit().value();
        let cost = txn_ctx.cost_to_use();
        let consumed = txn_ctx.consumed();
        let available = txn_ctx.available().unwrap_or(U512::zero());
        let gas_price = txn_ctx.gas_price();

        match self.refund_handling() {
            RefundHandling::NoRefund => None,
            RefundHandling::Burn { refund_ratio } => Some(HandleRefundMode::Burn {
                limit,
                gas_price,
                cost,
                consumed,
                source: Box::new(balance_identifier),
                ratio: refund_ratio,
                available,
            }),
            RefundHandling::Refund { refund_ratio } => {
                // in normal payment handling we put a temporary processing hold
                // on the paying purse rather than take the token up front.
                // thus, here we only want to determine the refund amount rather than
                // attempt to process a refund on something we haven't actually taken yet.
                // later in the flow when the processing hold is released and payment is
                // finalized we reduce the amount taken by the refunded amount. This avoids
                // the churn of taking the token up front via transfer (which writes
                // multiple permanent records) and then transfer some of it back (which
                // writes more permanent records).
                Some(HandleRefundMode::CalculateAmount {
                    limit,
                    gas_price,
                    consumed,
                    cost,
                    ratio: refund_ratio,
                    available,
                })
            }
        }
    }

    pub(crate) fn handle_refund_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        refund_mode: HandleRefundMode,
    ) -> HandleRefundRequest {
        HandleRefundRequest::new(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            txn_ctx.transaction_hash(),
            refund_mode,
        )
    }

    pub(crate) fn is_gas_hold(&self) -> bool {
        self.handling_settings.fee_handling.requires_hold()
    }

    pub(crate) fn fee_mode(&self, txn_ctx: &TransactionProcessContext) -> Option<HandleFeeMode> {
        let fee_amount = txn_ctx.fee_amount();

        let proposer = self.proposer().clone();
        let balance_identifier = match txn_ctx.balance_identifier() {
            Some(balance_identifier) => balance_identifier.clone(),
            None => return None,
        };

        match self.fee_handling() {
            FeeHandling::NoFee => Some(HandleFeeMode::credit(proposer, fee_amount, self.era_id())),
            FeeHandling::Burn => {
                let balance_identifier = match txn_ctx.balance_identifier() {
                    Some(balance_identifier) => balance_identifier.clone(),
                    None => return None,
                };
                Some(HandleFeeMode::burn(balance_identifier, Some(fee_amount)))
            }
            FeeHandling::PayToProposer => {
                let initiator_addr = txn_ctx.initiator_addr();
                Some(HandleFeeMode::pay(
                    initiator_addr
                        .account_hash()
                        .map(|_| Box::new(initiator_addr.clone())),
                    balance_identifier,
                    BalanceIdentifier::Public(*(proposer)),
                    fee_amount,
                ))
            }
            FeeHandling::Accumulate => {
                let initiator_addr = txn_ctx.initiator_addr();
                Some(HandleFeeMode::pay(
                    initiator_addr
                        .account_hash()
                        .map(|_| Box::new(initiator_addr.clone())),
                    balance_identifier,
                    BalanceIdentifier::Accumulate,
                    fee_amount,
                ))
            }
        }
    }

    pub(crate) fn handle_fee_request(
        &self,
        txn_ctx: &TransactionProcessContext,
        fee_mode: HandleFeeMode,
    ) -> HandleFeeRequest {
        HandleFeeRequest::new(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            txn_ctx.transaction_hash(),
            fee_mode,
        )
    }

    pub(crate) fn block_fee_request(&self) -> FeeRequest {
        FeeRequest::new(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            self.block_time(),
        )
    }

    pub(crate) fn block_rewards_request(
        &self,
        block_rewards: BTreeMap<PublicKey, Vec<U512>>,
    ) -> BlockRewardsRequest {
        BlockRewardsRequest::new(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            self.block_time(),
            block_rewards,
        )
    }

    pub(crate) fn step_request(&self, era_report: &InternalEraReport) -> StepRequest {
        let equivocators = &era_report.equivocators;
        let inactive_validators = &era_report.inactive_validators;
        let evict_items = inactive_validators
            .iter()
            .chain(equivocators)
            .map(|validator_id: &PublicKey| EvictItem::new(validator_id.clone()))
            .collect();

        let next_era_id = self.era_id().successor();
        let era_end_timestamp_millis = self.block_time_millis();

        StepRequest::new(
            self.native_runtime_config.clone(),
            self.state_root_hash,
            self.protocol_version,
            vec![],
            evict_items,
            next_era_id,
            era_end_timestamp_millis,
        )
    }

    pub(crate) fn era_validators_request(&self) -> EraValidatorsRequest {
        EraValidatorsRequest::new(self.state_root_hash)
    }

    pub(crate) fn prune_request(&self, keys_to_prune: Vec<Key>) -> PruneRequest {
        PruneRequest::new(self.state_root_hash, keys_to_prune)
    }

    pub(crate) fn into_outcome(
        self,
        proof: TrieMerkleProof<Key, StoredValue>,
    ) -> ExecuteBlockOutcome {
        let proposer = *self.proposer().clone();
        let ExecuteBlockContext {
            pre_state,
            executable_block,
            current_gas_price,
            last_switch_block_hash,
            approval_hashes,
            protocol_version,
            state_root_hash,
            execution_artifacts,
            step_outcome,
            timing,
            ..
        } = self;

        let era_id = executable_block.era_id;
        let next_era_id = era_id.successor();
        let maybe_next_era_validator_weights: Option<(BTreeMap<PublicKey, U512>, u8)> =
            match step_outcome.as_ref() {
                None => None,
                Some(effects_and_validators) => {
                    match effects_and_validators
                        .upcoming_era_validators
                        .get(&next_era_id)
                        .cloned()
                    {
                        Some(validators) => self
                            .next_era_gas_price
                            .map(|gas_price| (validators, gas_price)),
                        None => None,
                    }
                }
            };

        let era_end = match (
            executable_block.era_report,
            maybe_next_era_validator_weights,
        ) {
            (None, None) => None,
            (
                Some(InternalEraReport {
                    equivocators,
                    inactive_validators,
                }),
                Some((next_era_validator_weights, next_era_gas_price)),
            ) => {
                let rewards = executable_block.rewards.clone();
                Some(EraEndV2::new(
                    equivocators,
                    inactive_validators,
                    next_era_validator_weights,
                    rewards.unwrap_or_default(),
                    next_era_gas_price,
                ))
            }
            (maybe_era_report, maybe_next_era_validator_weights) => {
                let mut err_msg = "failed to create era_end".to_string();
                if maybe_era_report.is_none() {
                    err_msg = format!("era_end {}: maybe_era_report is none", era_id);
                }
                if maybe_next_era_validator_weights.is_none() {
                    err_msg = format!(
                        "era_end {}: maybe_next_era_validator_weights is none",
                        era_id
                    );
                }
                let elapsed = timing.pre_process().elapsed().as_secs_f64();
                return ExecuteBlockOutcome::FailedToCreateEraEnd {
                    elapsed,
                    err_msg,
                    maybe_era_report,
                    maybe_next_era_validator_weights,
                };
            }
        };

        let block = Arc::new(BlockV2::new(
            pre_state.parent_hash(),
            pre_state.parent_seed(),
            state_root_hash,
            executable_block.random_bit,
            era_end,
            executable_block.timestamp,
            executable_block.era_id,
            executable_block.height,
            protocol_version,
            proposer,
            executable_block.transaction_map,
            executable_block.rewarded_signatures,
            current_gas_price,
            last_switch_block_hash,
        ));

        let approvals_hashes =
            Box::new(ApprovalsHashes::new(*block.hash(), approval_hashes, proof));

        let ret = BlockAndExecutionArtifacts {
            block,
            approvals_hashes,
            execution_artifacts,
            step_outcome,
        };

        let elapsed = timing.pre_process().elapsed().as_secs_f64();
        ExecuteBlockOutcome::Success { ret, elapsed }
    }
}
