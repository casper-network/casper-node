use crate::components::fetcher::FetchItem;
use crate::contract_runtime::ExecutionPreState;
use crate::types::ExecutableBlock;
use casper_storage::data_access_layer::balance::BalanceHandling;
use casper_storage::data_access_layer::InsufficientBalanceHandling;
use casper_storage::system::runtime_native::Config as NativeRuntimeConfig;
use casper_types::bytesrepr::ToBytes;
use casper_types::{
    bytesrepr, ApprovalsHash, BlockHash, BlockTime, Chainspec, Digest, EraId, FeeHandling,
    ProtocolVersion, PublicKey, RefundHandling, Transaction,
};
use itertools::Itertools;
use serde::Serialize;
use std::time::Instant;
use thiserror::Error;

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
    insufficient_balance_handling: InsufficientBalanceHandling,
    balance_handling: BalanceHandling,
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
}

impl HandlingSettings {
    pub(crate) fn new(
        insufficient_balance_handling: InsufficientBalanceHandling,
        balance_handling: BalanceHandling,
        refund_handling: RefundHandling,
        fee_handling: FeeHandling,
    ) -> Self {
        HandlingSettings {
            insufficient_balance_handling,
            balance_handling,
            refund_handling,
            fee_handling,
        }
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
}

impl ExecuteBlockTiming {
    pub(crate) fn new(pre_process: Instant) -> Self {
        Self {
            pre_process,
            process: None,
            post_process: None,
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

    pub(crate) fn with_process(&mut self, process: Instant) -> &mut Self {
        self.process = Some(process);
        self
    }

    pub(crate) fn with_post_process(&mut self, post_process: Instant) -> &mut Self {
        self.post_process = Some(post_process);
        self
    }
}

pub(crate) struct ExecuteBlockContext {
    pre_state: ExecutionPreState,
    executable_block: ExecutableBlock,
    transaction_ids_checksum: Digest,
    approval_hashes: Vec<ApprovalsHash>,
    native_runtime_config: NativeRuntimeConfig,
    protocol_version: ProtocolVersion,
    activation_point_era_id: EraId,
    prune_batch_size: u64,
    addressable_entity_enabled: bool,
    handling_settings: HandlingSettings,
    timing: ExecuteBlockTiming,
}

impl ExecuteBlockContext {
    pub(crate) fn try_new(
        executable_block: &ExecutableBlock,
        execution_pre_state: &ExecutionPreState,
        chainspec: &Chainspec,
        next_era_gas_price: Option<u8>,
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
            .map(Transaction::fetch_id)
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

        // set up accounting variables / settings
        let insufficient_balance_handling = InsufficientBalanceHandling::HoldRemaining;
        let balance_handling = BalanceHandling::Available;
        let refund_handling = chainspec.core_config.refund_handling;
        let fee_handling = chainspec.core_config.fee_handling;
        let handling_settings = HandlingSettings::new(
            insufficient_balance_handling,
            balance_handling,
            refund_handling,
            fee_handling,
        );

        let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);

        Ok(ExecuteBlockContext {
            pre_state: execution_pre_state.clone(),
            executable_block: executable_block.clone(),
            transaction_ids_checksum,
            approval_hashes,
            protocol_version,
            activation_point_era_id,
            prune_batch_size,
            addressable_entity_enabled,
            native_runtime_config,
            handling_settings,
            timing,
        })
    }

    pub(crate) fn transaction_ids_checksum(&self) -> Digest {
        self.transaction_ids_checksum
    }

    pub(crate) fn approval_hashes(&self) -> Vec<ApprovalsHash> {
        self.approval_hashes.clone()
    }

    pub(crate) fn block_height(&self) -> u64 {
        self.executable_block.height
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

    pub(crate) fn proposer(&self) -> Box<PublicKey> {
        self.executable_block.proposer.clone()
    }

    pub(crate) fn era_id(&self) -> EraId {
        self.executable_block.era_id
    }

    pub(crate) fn native_runtime_config(&self) -> &NativeRuntimeConfig {
        &self.native_runtime_config
    }

    pub(crate) fn parent_block_hash(&self) -> BlockHash {
        self.pre_state.parent_hash()
    }

    pub(crate) fn parent_seed(&self) -> Digest {
        self.pre_state.parent_seed()
    }

    pub(crate) fn pre_state_root_hash(&self) -> Digest {
        self.pre_state.pre_state_root_hash()
    }

    pub(crate) fn insufficient_balance_handling(&self) -> InsufficientBalanceHandling {
        self.handling_settings.insufficient_balance_handling()
    }

    pub(crate) fn balance_handling(&self) -> BalanceHandling {
        self.handling_settings.balance_handling()
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

    pub(crate) fn elapsed(&self) -> f64 {
        // currently, nothing happens between start and setting pre-process time
        // if this changes in the future, the variable should be split.
        self.timing.pre_process().elapsed().as_secs_f64()
    }

    pub(crate) fn process_starting(&mut self) -> &mut Self {
        self.timing.with_process(Instant::now());
        self
    }

    pub(crate) fn post_process_starting(&mut self) -> &mut Self {
        self.timing.with_post_process(Instant::now());
        self
    }
}
