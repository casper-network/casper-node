use crate::types::transaction::{WasmV2Request, WasmV2Result};
use casper_executor_wasm::ExecutorV2;
use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{
    BlockInfo, ExecutionEngineV1, WasmV1Request, WasmV1Result,
};
use casper_executor_evm::{
    BlockContext as EvmBlockContext, BlockHashProvider as EvmBlockHashProvider,
    CallRequest as EvmExecutorCallRequest, CallValidation as EvmCallValidation, EvmExecutor,
    ExecuteKind as EvmExecuteKind, ExecuteRequest as EvmExecuteRequest,
};
use casper_storage::data_access_layer::BalanceResult;
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        mint::BurnRequest, AuctionMethod, BalanceHoldKind, BalanceHoldRequest, BalanceIdentifier,
        BalanceRequest, BiddingRequest, BlockGlobalRequest, BlockGlobalResult, BlockRewardsRequest,
        BlockRewardsResult, DataAccessLayer, EntryPointRequest, EntryPointResult,
        EraValidatorsRequest, EraValidatorsResult, EvictItem, FeeRequest, FeeResult, FlushRequest,
        HandleFeeMode, HandleFeeRequest, HandleRefundMode, HandleRefundRequest, ProofHandling,
        PruneRequest, PruneResult, StepRequest, StepResult, TransferRequest,
    },
    global_state::state::{
        lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, ScratchProvider,
        StateProvider, StateReader,
    },
    system::runtime_native::Config as NativeRuntimeConfig,
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    TrackingCopy,
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, Bytes, ToBytes, U32_SERIALIZED_LENGTH},
    evm::{Address as EvmAddress, IdentityInstruction as EvmIdentityInstruction},
    execution::{Effects, ExecutionResult, TransformKindV2, TransformV2},
    BlockHash, BlockHeader, BlockV2, CLValue, Chainspec, ChecksumRegistry, Digest, EntityAddr,
    EraEndV2, EraId, EvmTransactionError, FeeHandling, Gas, HashAddr, InvalidTransaction,
    InvalidTransactionV1, Key, ProtocolVersion, PublicKey, RefundHandling, StoredValue, TimeDiff,
    Transaction, TransactionEntryPoint, U512,
};

use super::{
    types::{SpeculativeExecutionResult, StepOutcome},
    utils::{self, calculate_prune_eras},
    BlockAndExecutionArtifacts, BlockExecutionError, ExecutionPreState, Metrics, StateResultError,
    APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
};
use crate::contract_runtime::types::{
    ExecuteBlockContext, ExecuteBlockContextError, InitialBalanceIdentifierResult,
};
use crate::{
    contract_runtime::types::{
        BalanceIdentifierResolution, EvmOriginResolution, ProcessRequest,
        StaticEvmBlockHashProvider, TransactionProcessContext,
    },
    types::{Chunkable, ExecutableBlock, InternalEraReport, MetaTransaction},
};

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_finalized_block(
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
    execution_engine_v1: &ExecutionEngineV1,
    execution_engine_v2: ExecutorV2,
    chainspec: &Chainspec,
    metrics: Option<Arc<Metrics>>,
    execution_pre_state: ExecutionPreState,
    _evm_block_hash_provider: &dyn EvmBlockHashProvider,
    executable_block: ExecutableBlock,
    key_block_height_for_activation_point: u64,
    current_gas_price: u8,
    next_era_gas_price: Option<u8>,
    last_switch_block_hash: Option<BlockHash>,
) -> Result<BlockAndExecutionArtifacts, BlockExecutionError> {
    let mut exec_ctx = match ExecuteBlockContext::try_new(
        &executable_block,
        &execution_pre_state,
        chainspec,
        next_era_gas_price,
        data_access_layer.enable_addressable_entity,
    ) {
        Ok(exec_ctx) => exec_ctx,
        Err(eb_err) => {
            return match eb_err {
                ExecuteBlockContextError::InvalidAESetting(chainspec_setting) => {
                    Err(BlockExecutionError::InvalidAESetting(chainspec_setting))
                }
                ExecuteBlockContextError::WrongBlockHeight {
                    executable_block,
                    execution_pre_state,
                } => Err(BlockExecutionError::WrongBlockHeight {
                    executable_block,
                    execution_pre_state,
                }),
                ExecuteBlockContextError::FailedToGetNewEraGasPrice { era_id } => {
                    Err(BlockExecutionError::FailedToGetNewEraGasPrice { era_id })
                }
                ExecuteBlockContextError::FailedToComputeApprovalsChecksum(bytesrepr_err) => Err(
                    BlockExecutionError::FailedToComputeApprovalsChecksum(bytesrepr_err),
                ),
            }
        }
    };

    // pre-processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_pre_processing
            .observe(exec_ctx.pre_process_elapsed());
    }

    exec_ctx.process_starting();

    let block_height = exec_ctx.block_height();
    let protocol_version = exec_ctx.protocol_version();
    let activation_point_era_id = exec_ctx.activation_point_era_id();
    let prune_batch_size = exec_ctx.prune_batch_size();
    let native_runtime_config = exec_ctx.native_runtime_config().clone();
    let block_time = exec_ctx.block_time();
    let proposer = exec_ctx.proposer();
    let era_id = exec_ctx.era_id();
    let parent_block_hash = exec_ctx.parent_block_hash();
    let parent_seed = exec_ctx.parent_seed();

    // mutable variables
    let mut state_root_hash = exec_ctx.pre_state_root_hash(); // initial state root is parent's state root
    let mut artifacts = Vec::with_capacity(executable_block.transactions.len());

    // NOTE this must occur prior to any block processing as subsequent logic
    // will refer to the values being written to GS.
    match data_access_layer.block_global(BlockGlobalRequest::set_block_info(
        state_root_hash,
        block_time,
        protocol_version,
        exec_ctx.addressable_entity_enabled(),
    )) {
        BlockGlobalResult::RootNotFound => {
            return Err(BlockExecutionError::RootNotFound(state_root_hash));
        }
        BlockGlobalResult::Failure(err) => {
            return Err(BlockExecutionError::BlockGlobal(format!("{:?}", err)));
        }
        BlockGlobalResult::Success {
            post_state_hash, ..
        } => {
            state_root_hash = post_state_hash;
        }
    }

    // scratch_state must be used for all processing and post-processing data
    // from here on out, until the effects are applied at the end.
    let scratch_state = data_access_layer.get_scratch_global_state();

    for txn in executable_block.transactions {
        let mut txn_process_ctx =
            TransactionProcessContext::try_new(&txn, chainspec, current_gas_price)
                .map_err(|err| BlockExecutionError::TransactionConversion(err.to_string()))?;

        let transaction_hash = txn_process_ctx.transaction_hash();

        let lane_id = txn_process_ctx.transaction_lane();
        if !chainspec.is_supported(lane_id) {
            // this transaction should not have been allowed in the block
            error!(
                %transaction_hash,
                %lane_id,
                "lane_id is currently not supported"
            );
            // record it and move on.
            artifacts.push(txn_process_ctx.into_execution_artifact());
            continue;
        }

        let initiator_addr = txn_process_ctx.initiator_addr().clone();

        let balance_identifier = {
            let ret = txn_initial_balance_identifier(
                &txn_process_ctx,
                &scratch_state,
                state_root_hash,
                protocol_version,
                exec_ctx.addressable_entity_enabled(),
            )
            .map_err(|err| {
                error!(
                    %transaction_hash,
                    ?err,
                    "failed to determine balance_identifier"
                );
                err
            })?;
            if let Some(state_err) = ret.state_result_error() {
                txn_process_ctx
                    .with_state_result_error(state_err.clone())
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
            }
            if let Some(evm_err) = ret.evm_transaction_error() {
                txn_process_ctx.with_evm_error(evm_err.clone());
            }
            if let Some(evm_resolution) = ret.evm_origin_resolution() {
                txn_process_ctx.with_evm_origin_resolution(Some(evm_resolution.clone()));
            }

            if let Some(bi) = ret.balance_identifier() {
                bi.clone()
            } else {
                // record it and move on.
                info!(
                    %transaction_hash,
                    "unknown initial balance identifier"
                );
                artifacts.push(txn_process_ctx.into_execution_artifact());
                continue;
            }
        };

        // we do a check for initial min balance to determine if we should proceed or not
        let initial_balance_result = scratch_state.balance(BalanceRequest::new(
            state_root_hash,
            protocol_version,
            balance_identifier.clone(),
            exec_ctx.balance_handling(),
            ProofHandling::NoProofs,
        ));

        if let BalanceResult::RootNotFound = initial_balance_result {
            return Err(BlockExecutionError::RootNotFound(state_root_hash));
        }

        txn_process_ctx.with_initial_balance_result(&initial_balance_result);

        let required_balance = match txn_process_ctx.cost_estimate() {
            Some(cost_estimate) => cost_estimate,
            None => {
                error!(
                    %transaction_hash,
                    "failed to determine cost_estimate"
                );
                // record it and move on.
                artifacts.push(txn_process_ctx.into_execution_artifact());
                continue;
            }
        };
        txn_process_ctx
            .with_is_sufficient_balance(initial_balance_result.is_sufficient(required_balance));
        txn_process_ctx.with_is_penalized(balance_identifier.is_penalty());
        txn_process_ctx.with_exec_attempt();

        // last point beyond which we don't charge
        // place a processing hold on the paying account to prevent double spend.
        let hold_amount = txn_process_ctx.cost_to_use();
        let hold_request = BalanceHoldRequest::new_processing_hold(
            state_root_hash,
            protocol_version,
            balance_identifier.clone(),
            hold_amount,
            exec_ctx.insufficient_balance_handling(),
        );
        let hold_result = scratch_state.balance_hold(hold_request);
        state_root_hash = scratch_state
            .commit_effects(state_root_hash, hold_result.effects().clone())
            .map_err(BlockExecutionError::Lmdb)?;
        txn_process_ctx
            .with_balance_hold_result(&hold_result)
            .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;

        let authorization_keys = txn_process_ctx.authorization_keys();
        // TODO: consider early skip if authorization_keys is empty

        let process_request = txn_process_ctx.process_request();
        trace!(%transaction_hash, %process_request, "process_request created");
        match process_request {
            ProcessRequest::NoExec => {
                // noop
                debug!(%transaction_hash, "no exec");
            }
            ProcessRequest::NoExecEvm {
                effective_gas_price,
            } => {
                txn_process_ctx.with_zero_cost();
                let outcome = casper_executor_evm::ExecutionOutcome::no_exec();
                txn_process_ctx.with_evm_execution_outcome(
                    outcome,
                    effective_gas_price,
                    Effects::new(),
                );

                debug!(%transaction_hash, "no exec evm");
            }
            ProcessRequest::NativeMint {
                session_args,
                entry_point,
            } => {
                // let transaction_args = txn_process_ctx.session_args();
                let runtime_args = session_args
                    .as_named()
                    .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
                // let entry_point = txn_process_ctx.entry_point();
                if let TransactionEntryPoint::Transfer = entry_point {
                    let transfer_result =
                        scratch_state.transfer(TransferRequest::with_runtime_args(
                            native_runtime_config.clone(),
                            state_root_hash,
                            protocol_version,
                            transaction_hash,
                            initiator_addr.clone(),
                            authorization_keys,
                            runtime_args.clone(),
                        ));
                    state_root_hash = scratch_state
                        .commit_effects(state_root_hash, transfer_result.effects().clone())
                        .map_err(BlockExecutionError::Lmdb)?;
                    txn_process_ctx
                        .with_gas_limit_consumed()
                        .with_transfer_result(transfer_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                } else if let TransactionEntryPoint::Burn = entry_point {
                    let burn_result = scratch_state.burn(BurnRequest::with_runtime_args(
                        native_runtime_config.clone(),
                        state_root_hash,
                        protocol_version,
                        transaction_hash,
                        initiator_addr.clone(),
                        authorization_keys,
                        runtime_args.clone(),
                    ));
                    state_root_hash = scratch_state
                        .commit_effects(state_root_hash, burn_result.effects().clone())
                        .map_err(BlockExecutionError::Lmdb)?;
                    txn_process_ctx
                        .with_gas_limit_consumed()
                        .with_burn_result(burn_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                } else {
                    txn_process_ctx.with_error_message(format!(
                        "Attempt to call unsupported native mint entrypoint: {}",
                        entry_point
                    ));
                }
            }
            ProcessRequest::NativeAuction {
                session_args,
                entry_point,
            } => {
                // let transaction_args = txn_process_ctx.session_args();
                let runtime_args = session_args
                    .as_named()
                    .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
                // let entry_point = txn_process_ctx.entry_point();
                match AuctionMethod::from_parts(entry_point, runtime_args, chainspec) {
                    Ok(auction_method) => {
                        let bidding_result = scratch_state.bidding(BiddingRequest::new(
                            native_runtime_config.clone(),
                            state_root_hash,
                            protocol_version,
                            transaction_hash,
                            initiator_addr.clone(),
                            authorization_keys,
                            auction_method,
                        ));
                        state_root_hash = scratch_state
                            .commit_effects(state_root_hash, bidding_result.effects().clone())
                            .map_err(BlockExecutionError::Lmdb)?;
                        txn_process_ctx
                            .with_gas_limit_consumed()
                            .with_bidding_result(bidding_result)
                            .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    }
                    Err(ame) => {
                        error!(
                            %transaction_hash,
                            ?ame,
                            "failed to determine auction method"
                        );
                        txn_process_ctx.with_auction_method_error(&ame);
                    }
                };
            }
            ProcessRequest::WasmV1 { session_input_data } => {
                let wasm_v1_start = Instant::now();
                match WasmV1Request::new_session(
                    BlockInfo::new(
                        state_root_hash,
                        block_time,
                        parent_block_hash,
                        block_height,
                        protocol_version,
                    ),
                    txn_process_ctx.gas_limit(),
                    &session_input_data,
                ) {
                    Ok(wasm_v1_request) => {
                        trace!(%transaction_hash, ?lane_id, ?wasm_v1_request, "able to get wasm v1 request");
                        let wasm_v1_result =
                            execution_engine_v1.execute(&scratch_state, wasm_v1_request);
                        trace!(%transaction_hash, ?lane_id, ?wasm_v1_result, "able to get wasm v1 result");
                        state_root_hash = scratch_state
                            .commit_effects(state_root_hash, wasm_v1_result.effects().clone())
                            .map_err(BlockExecutionError::Lmdb)?;
                        // note: consumed is scraped from wasm_v1_result along w/ other fields
                        txn_process_ctx
                            .with_wasm_v1_result(wasm_v1_result)
                            .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    }
                    Err(ire) => {
                        debug!(%transaction_hash, ?lane_id, ?ire, "unable to get wasm v1 request");
                        txn_process_ctx.with_invalid_wasm_v1_request(&ire);
                    }
                };
                if let Some(metrics) = metrics.as_ref() {
                    metrics
                        .exec_wasm_v1
                        .observe(wasm_v1_start.elapsed().as_secs_f64());
                }
            }
            ProcessRequest::WasmV2 { transaction_info } => {
                let wasm_v2_start = Instant::now();
                match WasmV2Request::new(
                    txn_process_ctx.gas_limit(),
                    chainspec.network_config.name.clone(),
                    state_root_hash,
                    parent_block_hash,
                    block_height,
                    transaction_info,
                ) {
                    Ok(wasm_v2_request) => {
                        match wasm_v2_request.execute(
                            &execution_engine_v2,
                            state_root_hash,
                            &scratch_state,
                        ) {
                            Ok(wasm_v2_result) => {
                                match &wasm_v2_result {
                                    WasmV2Result::Install(install_result) => {
                                        info!(
                                            contract_hash=base16::encode_lower(&install_result.smart_contract_addr()),
                                            pre_state_root_hash=%state_root_hash,
                                            post_state_root_hash=%install_result.post_state_hash(),
                                            "install contract result");
                                    }

                                    WasmV2Result::Execute(execute_result) => {
                                        info!(
                                            pre_state_root_hash=%state_root_hash,
                                            post_state_root_hash=%execute_result.post_state_hash(),
                                            host_error=?execute_result.host_error.as_ref(),
                                            "execute contract result");
                                    }
                                }

                                state_root_hash = wasm_v2_result.post_state_hash();
                                txn_process_ctx.with_wasm_v2_result(wasm_v2_result);
                            }
                            Err(wasm_v2_error) => {
                                txn_process_ctx.with_wasm_v2_error(wasm_v2_error);
                            }
                        }
                    }
                    Err(ire) => {
                        debug!(%transaction_hash, ?lane_id, ?ire, "unable to get wasm v2 request");
                        txn_process_ctx.with_invalid_wasm_v2_request(ire);
                    }
                }
                if let Some(metrics) = metrics.as_ref() {
                    metrics
                        .exec_wasm_v2
                        .observe(wasm_v2_start.elapsed().as_secs_f64());
                }
            }
            ProcessRequest::EvmV1 {
                evm_txn,
                base_fee_wei,
                effective_gas_price,
                block_gas_limit,
            } => {
                let evm_v1_start = Instant::now();
                let block_context = EvmBlockContext::new(
                    block_height,
                    block_time,
                    block_gas_limit,
                    base_fee_wei,
                    EvmAddress::from_block_proposer_public_key(&proposer),
                );
                let request = EvmExecuteRequest {
                    block: block_context,
                    kind: EvmExecuteKind::Transaction(Box::new(evm_txn)),
                };

                let mut tracking_copy = scratch_state
                    .tracking_copy(state_root_hash)
                    .map_err(BlockExecutionError::Lmdb)?
                    .ok_or(BlockExecutionError::RootNotFound(state_root_hash))?;

                // TODO: move origin resolution to before txn process request
                // if let Some(resolution) = &evm_origin_resolution {
                //     // Apply the deferred bridge/account creation only now,
                //     // after balance preconditions have allowed execution.
                //     // This keeps rejected EVM transactions from mutating
                //     // identity state and makes the identity write atomic
                //     // with the revm state transition below.
                //     apply_evm_identity_plan(
                //         &mut tracking_copy,
                //         protocol_version,
                //         resolution.identity_plan(),
                //     )?;
                // }
                // apply_evm_proposer_identity(&mut tracking_copy, protocol_version, &proposer)?;

                let outcome = EvmExecutor::new(chainspec.evm_config)
                    .execute(&mut tracking_copy, request)
                    .map_err(|error| {
                        BlockExecutionError::TransactionConversion(error.to_string())
                    })?;
                let execution_effects = tracking_copy.effects();
                state_root_hash = scratch_state
                    .commit_effects(state_root_hash, execution_effects.clone())
                    .map_err(BlockExecutionError::Lmdb)?;

                txn_process_ctx.with_evm_execution_outcome(
                    outcome,
                    effective_gas_price,
                    execution_effects,
                );
                if let Some(metrics) = metrics.as_ref() {
                    metrics
                        .exec_evm_v1
                        .observe(evm_v1_start.elapsed().as_secs_f64());
                }
            }
            ProcessRequest::Unknown => {
                // this should be unreachable
                unreachable!("Unknown transaction execution target");
            }
        }

        // clear all holds on the balance_identifier purse before payment processing
        {
            let hold_request = BalanceHoldRequest::new_clear(
                state_root_hash,
                protocol_version,
                BalanceHoldKind::All,
                balance_identifier.clone(),
            );
            let hold_result = scratch_state.balance_hold(hold_request);
            state_root_hash = scratch_state
                .commit_effects(state_root_hash, hold_result.effects().clone())
                .map_err(BlockExecutionError::Lmdb)?;
            txn_process_ctx
                .with_balance_hold_result(&hold_result)
                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
        }

        // handle refunds per the chainspec determined setting.
        let refund_amount = {
            let consumed = if balance_identifier.is_penalty() || txn_process_ctx.has_error() {
                txn_process_ctx.cost_to_use() // no refund for penalty
            } else {
                txn_process_ctx.consumed()
            };

            let available = txn_process_ctx.available().unwrap_or(U512::zero());

            let refund_mode = match exec_ctx.refund_handling() {
                RefundHandling::NoRefund => None,
                RefundHandling::Burn { refund_ratio } => {
                    let (limit, cost, gas_price) = txn_process_ctx.refund_amounts();
                    Some(HandleRefundMode::Burn {
                        limit,
                        gas_price,
                        cost,
                        consumed,
                        source: Box::new(balance_identifier.clone()),
                        ratio: refund_ratio,
                        available,
                    })
                }
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
                    let (limit, cost, gas_price) = txn_process_ctx.refund_amounts();
                    Some(HandleRefundMode::CalculateAmount {
                        limit,
                        gas_price,
                        consumed,
                        cost,
                        ratio: refund_ratio,
                        available,
                    })
                }
            };
            match refund_mode {
                Some(refund_mode) => {
                    let handle_refund_request = HandleRefundRequest::new(
                        native_runtime_config.clone(),
                        state_root_hash,
                        protocol_version,
                        transaction_hash,
                        refund_mode,
                    );
                    let handle_refund_result = scratch_state.handle_refund(handle_refund_request);
                    let refunded_amount = handle_refund_result.refund_amount();
                    state_root_hash = scratch_state
                        .commit_effects(state_root_hash, handle_refund_result.effects().clone())
                        .map_err(BlockExecutionError::Lmdb)?;
                    txn_process_ctx
                        .with_handle_refund_result(&handle_refund_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;

                    refunded_amount
                }
                None => U512::zero(),
            }
        };
        txn_process_ctx.with_refund_amount(refund_amount);

        // take the lower of the difference between cost - refund OR available
        let fee_amount = txn_process_ctx
            .cost_to_use()
            .saturating_sub(refund_amount)
            .min(txn_process_ctx.available().unwrap_or(U512::zero()));

        // handle fees per the chainspec determined setting.
        let handle_fee_result = match exec_ctx.fee_handling() {
            FeeHandling::NoFee => {
                // in this mode, a gas hold is placed on the payer's purse.
                let hold_request = BalanceHoldRequest::new_gas_hold(
                    state_root_hash,
                    protocol_version,
                    balance_identifier,
                    fee_amount,
                    exec_ctx.insufficient_balance_handling(),
                );
                let hold_result = scratch_state.balance_hold(hold_request);
                state_root_hash = scratch_state
                    .commit_effects(state_root_hash, hold_result.effects().clone())
                    .map_err(BlockExecutionError::Lmdb)?;
                txn_process_ctx
                    .with_balance_hold_result(&hold_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::credit(proposer.clone(), fee_amount, era_id),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
            FeeHandling::Burn => {
                // in this mode, the fee portion is burned.
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::burn(balance_identifier, Some(fee_amount)),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
            FeeHandling::PayToProposer => {
                // in this mode, the consumed gas is paid as a fee to the block proposer
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::pay(
                        initiator_addr
                            .account_hash()
                            .map(|_| Box::new(initiator_addr.clone())),
                        balance_identifier,
                        BalanceIdentifier::Public(*(proposer.clone())),
                        fee_amount,
                    ),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
            FeeHandling::Accumulate => {
                // in this mode, consumed gas is accumulated into a single purse
                // for later distribution
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::pay(
                        initiator_addr
                            .account_hash()
                            .map(|_| Box::new(initiator_addr.clone())),
                        balance_identifier,
                        BalanceIdentifier::Accumulate,
                        fee_amount,
                    ),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
        };

        state_root_hash = scratch_state
            .commit_effects(state_root_hash, handle_fee_result.effects().clone())
            .map_err(BlockExecutionError::Lmdb)?;

        txn_process_ctx
            .with_handle_fee_result(&handle_fee_result)
            .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;

        if let Some(err_msg) = txn_process_ctx.error_message() {
            debug!(%transaction_hash, ?err_msg, "transaction error");
        }

        artifacts.push(txn_process_ctx.into_execution_artifact());
    }

    // transaction processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_tnx_processing
            .observe(exec_ctx.process_elapsed());
    }

    exec_ctx.post_process_starting();

    {
        // the canonical full set of approvals and metadata must be historically verifiable.
        // to allow this, we must calculate and store checksums for approvals and execution effects
        //   across all transactions in the block.
        // block synchronization uses these checksums to ensure correct complete block data.
        let execution_results_checksum = compute_execution_results_checksum(
            artifacts.iter().map(|artifact| &artifact.execution_result),
        )?;
        let transaction_ids_checksum = exec_ctx.transaction_ids_checksum();

        let mut checksum_registry = ChecksumRegistry::new();
        checksum_registry.insert(APPROVALS_CHECKSUM_NAME, transaction_ids_checksum);
        checksum_registry.insert(EXECUTION_RESULTS_CHECKSUM_NAME, execution_results_checksum);

        let mut effects = Effects::new();
        effects.push(TransformV2::new(
            Key::ChecksumRegistry,
            TransformKindV2::Write(
                CLValue::from_t(checksum_registry)
                    .map_err(BlockExecutionError::ChecksumRegistryToCLValue)?
                    .into(),
            ),
        ));
        scratch_state
            .commit_effects(state_root_hash, effects)
            .map_err(BlockExecutionError::Lmdb)?;
    };

    if let Some(metrics) = metrics.as_ref() {
        metrics
            .txn_approvals_hashes_calculation
            .observe(exec_ctx.post_process_elapsed());
    }

    // Pay out  ̶b̶l̶o̶c̶k̶ e͇r͇a͇ rewards
    // NOTE: despite the name, these rewards are currently paid out per ERA not per BLOCK
    // at one point, they were going to be paid out per block (and might be in the future)
    // but it ended up settling on per era. the behavior is driven by Some / None
    // thus if in future the calling logic passes rewards per block it should just work as is.
    // This auto-commits.
    if let Some(rewards) = &executable_block.rewards {
        let block_rewards_payout_start = Instant::now();
        // Pay out block fees, if relevant. This auto-commits
        {
            let fee_req = FeeRequest::new(
                native_runtime_config.clone(),
                state_root_hash,
                protocol_version,
                block_time,
            );
            debug!(?fee_req, "distributing fees");
            match scratch_state.distribute_fees(fee_req) {
                FeeResult::RootNotFound => {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                }
                FeeResult::Failure(fer) => return Err(BlockExecutionError::DistributeFees(fer)),
                FeeResult::Success {
                    post_state_hash, ..
                } => {
                    debug!("fee distribution success");
                    state_root_hash = post_state_hash;
                }
            }
        }

        let rewards_req = BlockRewardsRequest::new(
            native_runtime_config.clone(),
            state_root_hash,
            protocol_version,
            block_time,
            rewards.clone(),
        );
        debug!(?rewards_req, "distributing rewards");
        match scratch_state.distribute_block_rewards(rewards_req) {
            BlockRewardsResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash));
            }
            BlockRewardsResult::Failure(bre) => {
                return Err(BlockExecutionError::DistributeBlockRewards(bre));
            }
            BlockRewardsResult::Success {
                post_state_hash, ..
            } => {
                debug!("rewards distribution success");
                state_root_hash = post_state_hash;
            }
        }
        if let Some(metrics) = metrics.as_ref() {
            metrics
                .block_rewards_payout
                .observe(block_rewards_payout_start.elapsed().as_secs_f64());
        }
    }

    // if era report is some, this is a switch block. a series of end-of-era extra processing must
    // transpire before this block is entirely finished.
    let step_outcome = if let Some(era_report) = &executable_block.era_report {
        // step processing starts now
        let step_processing_start = Instant::now();

        debug!("committing step");
        let step_effects = match commit_step(
            native_runtime_config.clone(),
            &scratch_state,
            metrics.clone(),
            protocol_version,
            state_root_hash,
            era_report.clone(),
            block_time.value(),
            executable_block.era_id.successor(),
        ) {
            StepResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash));
            }
            StepResult::Failure(err) => return Err(BlockExecutionError::Step(err)),
            StepResult::Success {
                effects,
                post_state_hash,
                ..
            } => {
                state_root_hash = post_state_hash;
                effects
            }
        };
        debug!("step committed");

        let era_validators_req = EraValidatorsRequest::new(state_root_hash);
        let era_validators_result = data_access_layer.era_validators(era_validators_req);

        let upcoming_era_validators = match era_validators_result {
            EraValidatorsResult::RootNotFound => {
                panic!("root not found");
            }
            EraValidatorsResult::AuctionNotFound => {
                panic!("auction not found");
            }
            EraValidatorsResult::ValueNotFound(msg) => {
                panic!("validator snapshot not found: {}", msg);
            }
            EraValidatorsResult::Failure(tce) => {
                return Err(BlockExecutionError::GetEraValidators(tce));
            }
            EraValidatorsResult::Success { era_validators } => era_validators,
        };

        // step processing is finished
        if let Some(metrics) = metrics.as_ref() {
            metrics
                .exec_block_step_processing
                .observe(step_processing_start.elapsed().as_secs_f64());
        }
        Some(StepOutcome {
            step_effects,
            upcoming_era_validators,
        })
    } else {
        None
    };

    // Pruning -- this is orthogonal to the contents of the block, but we deliberately do it
    // at the end to avoid a read ordering issue during block execution.
    if let Some(previous_block_height) = block_height.checked_sub(1) {
        if let Some(keys_to_prune) = calculate_prune_eras(
            activation_point_era_id,
            key_block_height_for_activation_point,
            previous_block_height,
            prune_batch_size,
        ) {
            let pruning_start = Instant::now();

            let first_key = keys_to_prune.first().copied();
            let last_key = keys_to_prune.last().copied();
            info!(
                previous_block_height,
                %key_block_height_for_activation_point,
                %state_root_hash,
                first_key=?first_key,
                last_key=?last_key,
                "commit prune: preparing prune config"
            );
            let request = PruneRequest::new(state_root_hash, keys_to_prune);
            match scratch_state.prune(request) {
                PruneResult::RootNotFound => {
                    error!(
                        previous_block_height,
                        %state_root_hash,
                        "commit prune: root not found"
                    );
                    panic!(
                        "Root {} not found while performing a prune.",
                        state_root_hash
                    );
                }
                PruneResult::MissingKey => {
                    warn!(
                        previous_block_height,
                        %state_root_hash,
                        "commit prune: key does not exist"
                    );
                }
                PruneResult::Success {
                    post_state_hash, ..
                } => {
                    info!(
                        previous_block_height,
                        %key_block_height_for_activation_point,
                        %state_root_hash,
                        %post_state_hash,
                        first_key=?first_key,
                        last_key=?last_key,
                        "commit prune: success"
                    );
                    state_root_hash = post_state_hash;
                }
                PruneResult::Failure(tce) => {
                    error!(?tce, "commit prune: failure");
                    return Err(tce.into());
                }
            }
            if let Some(metrics) = metrics.as_ref() {
                metrics
                    .pruning_time
                    .observe(pruning_start.elapsed().as_secs_f64());
            }
        }
    }

    {
        let database_write_start = Instant::now();
        // Finally, the new state-root-hash from the cumulative changes to global state is
        // returned when they are written to LMDB.
        state_root_hash = data_access_layer
            .write_scratch_to_db(state_root_hash, scratch_state)
            .map_err(BlockExecutionError::Lmdb)?;
        if let Some(metrics) = metrics.as_ref() {
            metrics
                .scratch_lmdb_write_time
                .observe(database_write_start.elapsed().as_secs_f64());
        }

        // Flush once, after all data mutation.
        let database_flush_start = Instant::now();
        let flush_req = FlushRequest::new();
        let flush_result = data_access_layer.flush(flush_req);
        if let Err(gse) = flush_result.as_error() {
            error!("failed to flush lmdb");
            return Err(BlockExecutionError::Lmdb(gse));
        }
        if let Some(metrics) = metrics.as_ref() {
            metrics
                .database_flush_time
                .observe(database_flush_start.elapsed().as_secs_f64());
        }
    }

    // the rest of this is post process, picking out data bits to return to caller
    let next_era_id = executable_block.era_id.successor();
    let maybe_next_era_validator_weights: Option<(BTreeMap<PublicKey, U512>, u8)> =
        match step_outcome.as_ref() {
            None => None,
            Some(effects_and_validators) => {
                match effects_and_validators
                    .upcoming_era_validators
                    .get(&next_era_id)
                    .cloned()
                {
                    Some(validators) => next_era_gas_price.map(|gas_price| (validators, gas_price)),
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
        ) => Some(EraEndV2::new(
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            executable_block.rewards.unwrap_or_default(),
            next_era_gas_price,
        )),
        (maybe_era_report, maybe_next_era_validator_weights) => {
            if maybe_era_report.is_none() {
                error!(
                    "era_end {}: maybe_era_report is none",
                    executable_block.era_id
                );
            }
            if maybe_next_era_validator_weights.is_none() {
                error!(
                    "era_end {}: maybe_next_era_validator_weights is none",
                    executable_block.era_id
                );
            }
            return Err(BlockExecutionError::FailedToCreateEraEnd {
                maybe_era_report,
                maybe_next_era_validator_weights,
            });
        }
    };

    let block = Arc::new(BlockV2::new(
        parent_block_hash,
        parent_seed,
        state_root_hash,
        executable_block.random_bit,
        era_end,
        executable_block.timestamp,
        executable_block.era_id,
        block_height,
        protocol_version,
        (*proposer).clone(),
        executable_block.transaction_map,
        executable_block.rewarded_signatures,
        current_gas_price,
        last_switch_block_hash,
    ));

    let proof_of_checksum_registry = match data_access_layer
        .tracking_copy(state_root_hash)
        .map_err(BlockExecutionError::Lmdb)?
    {
        Some(tc) => match tc
            .reader()
            .read_with_proof(&Key::ChecksumRegistry)
            .map_err(BlockExecutionError::Lmdb)?
        {
            Some(proof) => proof,
            None => return Err(BlockExecutionError::MissingChecksumRegistry),
        },
        None => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
    };

    let transaction_approvals_hashes = exec_ctx.approval_hashes();
    let approvals_hashes = Box::new(ApprovalsHashes::new(
        *block.hash(),
        transaction_approvals_hashes,
        proof_of_checksum_registry,
    ));

    // processing is finished now
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_post_processing
            .observe(exec_ctx.post_process_elapsed());
        metrics.exec_block_total.observe(exec_ctx.elapsed());
    }

    Ok(BlockAndExecutionArtifacts {
        block,
        approvals_hashes,
        execution_artifacts: artifacts,
        step_outcome,
    })
}

/// Execute the transaction without committing the effects.
/// Intended to be used for discovery operations on read-only nodes.
///
/// Returns effects of the execution.
pub(super) fn speculatively_execute<S>(
    state_provider: &S,
    chainspec: &Chainspec,
    execution_engine_v1: &ExecutionEngineV1,
    block_header: BlockHeader,
    block_hashes: BTreeMap<u64, BlockHash>,
    input_transaction: Transaction,
) -> SpeculativeExecutionResult
where
    S: StateProvider,
{
    let gas_price = 1; // run speculative with flat cost
    let maybe_transaction =
        MetaTransaction::new_from_txn_with_price(&input_transaction, chainspec, gas_price);
    if let Err(error) = maybe_transaction {
        return SpeculativeExecutionResult::invalid_transaction(error);
    }
    let transaction = maybe_transaction.unwrap();
    if let Err(error) =
        transaction.is_config_compliant(chainspec, TimeDiff::ZERO, transaction.timestamp())
    {
        return SpeculativeExecutionResult::invalid_transaction(error);
    }
    let state_root_hash = block_header.state_root_hash();
    let parent_block_hash = block_header.block_hash();
    let block_height = block_header.height();
    let block_time = block_header
        .timestamp()
        .saturating_add(chainspec.core_config.minimum_block_time);

    if transaction.is_deploy_transaction() {
        let gas_limit = match input_transaction.gas_limit(chainspec, transaction.transaction_lane())
        {
            Ok(gas_limit) => gas_limit,
            Err(_) => {
                return SpeculativeExecutionResult::invalid_gas_limit(input_transaction);
            }
        };
        if transaction.is_native() {
            let limit = Gas::from(chainspec.system_costs_config.mint_costs().transfer);
            let protocol_version = chainspec.protocol_version();
            let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);
            let transaction_hash = transaction.hash();
            let initiator_addr = transaction.initiator_addr();
            let authorization_keys = transaction.authorization_keys();
            let runtime_args = match transaction.session_args().as_named() {
                Some(runtime_args) => runtime_args.clone(),
                None => {
                    return SpeculativeExecutionResult::InvalidTransaction(InvalidTransaction::V1(
                        InvalidTransactionV1::ExpectedNamedArguments,
                    ));
                }
            };

            let result = state_provider.transfer(TransferRequest::with_runtime_args(
                native_runtime_config.clone(),
                *state_root_hash,
                protocol_version,
                transaction_hash,
                initiator_addr.clone(),
                authorization_keys,
                runtime_args,
            ));
            SpeculativeExecutionResult::WasmV1(Box::new(utils::spec_exec_from_transfer_result(
                limit,
                result,
                block_header.block_hash(),
            )))
        } else {
            let block_info = BlockInfo::new(
                *state_root_hash,
                block_time.into(),
                parent_block_hash,
                block_height,
                execution_engine_v1.config().protocol_version(),
            );
            let session_input_data = transaction.to_session_input_data();
            let wasm_v1_result = match WasmV1Request::new_session_speculative(
                block_info,
                gas_limit,
                &session_input_data,
            ) {
                Ok(wasm_v1_request) => execution_engine_v1.execute(state_provider, wasm_v1_request),
                Err(error) => WasmV1Result::invalid_executable_item(gas_limit, error),
            };
            SpeculativeExecutionResult::WasmV1(Box::new(utils::spec_exec_from_wasm_v1_result(
                wasm_v1_result,
                block_header.block_hash(),
            )))
        }
    } else if transaction.is_wasm() {
        let gas_limit = match input_transaction.gas_limit(chainspec, transaction.transaction_lane())
        {
            Ok(gas_limit) => gas_limit,
            Err(_) => {
                return SpeculativeExecutionResult::invalid_gas_limit(input_transaction);
            }
        };
        let block_info = BlockInfo::new(
            *state_root_hash,
            block_time.into(),
            parent_block_hash,
            block_height,
            execution_engine_v1.config().protocol_version(),
        );
        let session_input_data = transaction.to_session_input_data();
        let wasm_v1_result = match WasmV1Request::new_session_speculative(
            block_info,
            gas_limit,
            &session_input_data,
        ) {
            Ok(wasm_v1_request) => execution_engine_v1.execute(state_provider, wasm_v1_request),
            Err(error) => WasmV1Result::invalid_executable_item(gas_limit, error),
        };
        SpeculativeExecutionResult::WasmV1(Box::new(utils::spec_exec_from_wasm_v1_result(
            wasm_v1_result,
            block_header.block_hash(),
        )))
    } else if let Some(evm_transaction) = transaction.as_evm() {
        speculatively_execute_evm(
            state_provider,
            chainspec,
            block_header,
            block_hashes,
            evm_transaction,
        )
    } else {
        // TODO: placeholder error
        SpeculativeExecutionResult::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::CannotCalculateFieldsHash,
        ))
    }
}

fn speculatively_execute_evm<S>(
    state_provider: &S,
    chainspec: &Chainspec,
    block_header: BlockHeader,
    block_hashes: BTreeMap<u64, BlockHash>,
    evm_transaction: &casper_types::EvmTransaction,
) -> SpeculativeExecutionResult
where
    S: StateProvider,
{
    if !chainspec.evm_config.enabled {
        return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
            EvmTransactionError::Disabled,
        ));
    }
    if evm_transaction.gas_limit() > chainspec.evm_config.block_gas_limit {
        return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
            EvmTransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit: evm_transaction.gas_limit(),
                block_gas_limit: chainspec.evm_config.block_gas_limit,
            },
        ));
    }

    let state_root_hash = block_header.state_root_hash();
    let mut tracking_copy = match state_provider.tracking_copy(*state_root_hash) {
        Ok(Some(tracking_copy)) => tracking_copy,
        Ok(None) => {
            return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
                EvmTransactionError::Decode(format!("state root {state_root_hash} not found")),
            ))
        }
        Err(error) => {
            return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
                EvmTransactionError::Decode(format!(
                    "failed to check out EVM speculative execution state: {error}"
                )),
            ))
        }
    };
    let block_time = block_header
        .timestamp()
        .saturating_add(chainspec.core_config.minimum_block_time);

    let base_fee = u128::from(chainspec.evm_config.base_fee);
    let wei_per_mote = u128::from(chainspec.evm_config.wei_per_mote);
    let base_fee_wei = base_fee * wei_per_mote;
    let block_context = EvmBlockContext {
        number: block_header.height(),
        timestamp: block_time.millis() / 1000,
        beneficiary: EvmAddress::ZERO,
        block_gas_limit: chainspec.evm_config.block_gas_limit,
        base_fee_wei,
    };
    let kind = if evm_transaction.is_unsigned_call() {
        EvmExecuteKind::Call(EvmExecutorCallRequest {
            from: evm_transaction.from(),
            to: evm_transaction.to(),
            value: evm_transaction.value(),
            input: evm_transaction.input().to_vec(),
            gas_limit: evm_transaction.gas_limit(),
            gas_price: base_fee_wei,
            nonce: evm_transaction.nonce(),
            validation: EvmCallValidation::UncheckedSimulation,
        })
    } else {
        EvmExecuteKind::Transaction(Box::new(evm_transaction.clone()))
    };
    let execute_request = EvmExecuteRequest {
        block: block_context,
        kind,
    };
    let block_hash_provider = StaticEvmBlockHashProvider::new(block_hashes);
    let outcome = match EvmExecutor::new(chainspec.evm_config).execute_with_block_hash_provider(
        &mut tracking_copy,
        execute_request,
        &block_hash_provider,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
                EvmTransactionError::Decode(error.to_string()),
            ))
        }
    };
    let effects = tracking_copy.effects();
    let effective_gas_price = if evm_transaction.is_unsigned_call() {
        base_fee_wei
    } else {
        evm_transaction.effective_gas_price(base_fee_wei)
    };
    let receipt = outcome.to_receipt(effective_gas_price);
    let error = receipt.status.message().map(str::to_string);
    SpeculativeExecutionResult::Evm(Box::new(
        casper_binary_port::EvmSpeculativeExecutionResult::new(
            block_header.block_hash(),
            Gas::new(evm_transaction.gas_limit()),
            Gas::new(outcome.gas_used),
            effects,
            error,
            receipt,
            Bytes::from(outcome.output),
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
fn commit_step(
    native_runtime_config: NativeRuntimeConfig,
    scratch_state: &ScratchGlobalState,
    maybe_metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    state_hash: Digest,
    InternalEraReport {
        equivocators,
        inactive_validators,
    }: InternalEraReport,
    era_end_timestamp_millis: u64,
    next_era_id: EraId,
) -> StepResult {
    // Both inactive validators and equivocators are evicted
    let evict_items = inactive_validators
        .into_iter()
        .chain(equivocators)
        .map(EvictItem::new)
        .collect();

    let step_request = StepRequest::new(
        native_runtime_config,
        state_hash,
        protocol_version,
        vec![], // <-- casper mainnet currently does not slash
        evict_items,
        next_era_id,
        era_end_timestamp_millis,
    );

    // Commit the step.
    let start = Instant::now();
    let result = scratch_state.step(step_request);
    debug_assert!(result.is_success(), "{:?}", result);
    if let Some(metrics) = maybe_metrics {
        let elapsed = start.elapsed().as_secs_f64();
        metrics.commit_step.observe(elapsed);
        metrics.latest_commit_step.set(elapsed);
    }
    trace!(?result, "step response");
    result
}

/// Computes the checksum of the given set of execution results.
///
/// This will either be a simple hash of the bytesrepr-encoded results (in the case that the
/// serialized results are not greater than `ChunkWithProof::CHUNK_SIZE_BYTES`), or otherwise will
/// be a Merkle root hash of the chunks derived from the serialized results.
pub(crate) fn compute_execution_results_checksum<'a>(
    execution_results_iter: impl Iterator<Item = &'a ExecutionResult> + Clone,
) -> Result<Digest, BlockExecutionError> {
    // Serialize the execution results as if they were `Vec<ExecutionResult>`.
    let serialized_length = U32_SERIALIZED_LENGTH
        + execution_results_iter
            .clone()
            .map(|exec_result| exec_result.serialized_length())
            .sum::<usize>();
    let mut serialized = vec![];
    serialized
        .try_reserve_exact(serialized_length)
        .map_err(|_| {
            BlockExecutionError::FailedToComputeApprovalsChecksum(bytesrepr::Error::OutOfMemory)
        })?;
    let item_count: u32 = execution_results_iter
        .clone()
        .count()
        .try_into()
        .map_err(|_| {
            BlockExecutionError::FailedToComputeApprovalsChecksum(
                bytesrepr::Error::NotRepresentable,
            )
        })?;
    item_count
        .write_bytes(&mut serialized)
        .map_err(BlockExecutionError::FailedToComputeExecutionResultsChecksum)?;
    for execution_result in execution_results_iter {
        execution_result
            .write_bytes(&mut serialized)
            .map_err(BlockExecutionError::FailedToComputeExecutionResultsChecksum)?;
    }

    // Now hash the serialized execution results, using the `Chunkable` trait's `hash` method to
    // chunk if required.
    serialized.hash().map_err(|_| {
        BlockExecutionError::FailedToComputeExecutionResultsChecksum(bytesrepr::Error::OutOfMemory)
    })
}

// *************** EVM RELATED *****************

fn account_main_purse<R>(
    tracking_copy: &mut TrackingCopy<R>,
    protocol_version: ProtocolVersion,
    account_hash: AccountHash,
) -> Result<Option<casper_types::URef>, BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    // Runtime footprints cover both legacy `StoredValue::Account` accounts and
    // addressable-entity-backed accounts, so this is the authoritative account
    // existence check for identity linking.
    match tracking_copy.runtime_footprint_by_account_hash(protocol_version, account_hash) {
        Ok((_, entity)) => entity
            .main_purse()
            .map(Some)
            .ok_or_else(|| BlockExecutionError::PaymentError("missing account main purse".into())),
        Err(TrackingCopyError::KeyNotFound(_)) => Ok(None),
        Err(error) => Err(BlockExecutionError::PaymentError(error.to_string())),
    }
}

fn invoked_contract_will_pay(
    state_provider: &ScratchGlobalState,
    state_root_hash: Digest,
    contract_address: Option<(HashAddr, String)>,
) -> Result<Option<EntityAddr>, StateResultError> {
    if let Some((hash_addr, entry_point_name)) = contract_address {
        let entity_addr = EntityAddr::new_smart_contract(hash_addr);
        let entry_point_request =
            EntryPointRequest::new(state_root_hash, entry_point_name, hash_addr);
        let entry_point_response = state_provider.entry_point(entry_point_request);
        match entry_point_response {
            EntryPointResult::RootNotFound => Err(StateResultError::RootNotFound),
            EntryPointResult::ValueNotFound(msg) => Err(StateResultError::ValueNotFound(msg)),
            EntryPointResult::Failure(tce) => Err(StateResultError::Failure(tce)),
            EntryPointResult::Success { entry_point } => {
                if entry_point.will_pay_direct_invocation() {
                    Ok(Some(entity_addr))
                } else {
                    Ok(None)
                }
            }
        }
    } else {
        Ok(None)
    }
}

fn txn_initial_balance_identifier(
    ctx: &TransactionProcessContext,
    state_provider: &ScratchGlobalState,
    state_root_hash: Digest,
    protocol_version: ProtocolVersion,
    addressable_entity_enabled: bool,
) -> Result<InitialBalanceIdentifierResult, BlockExecutionError> {
    let transaction_hash = ctx.transaction_hash();
    let initiator_addr = ctx.initiator_addr().clone();

    match ctx.balance_identifier_resolution() {
        Ok(resolution) => {
            match resolution {
                BalanceIdentifierResolution::Identifier(bi) => {
                    Ok(InitialBalanceIdentifierResult::Identifier(bi))
                }
                BalanceIdentifierResolution::CheckContractPay(default_bi) => {
                    if !addressable_entity_enabled {
                        // the initiating account pays using its main purse
                        trace!(%transaction_hash, "account session");
                        Ok(InitialBalanceIdentifierResult::Identifier(default_bi))
                    } else {
                        match invoked_contract_will_pay(
                            state_provider,
                            state_root_hash,
                            ctx.contract_direct_address(),
                        ) {
                            Ok(Some(entity_addr)) => {
                                let entity_bi = BalanceIdentifier::Entity(entity_addr);
                                Ok(InitialBalanceIdentifierResult::Identifier(entity_bi))
                            }
                            Ok(None) => {
                                // the initiating account pays using its main purse
                                trace!(%transaction_hash, "direct invocation with account payment");
                                Ok(InitialBalanceIdentifierResult::Identifier(default_bi))
                            }
                            Err(state_err) => {
                                trace!(%transaction_hash, "failed to resolve contract self payment");
                                let penalized_bi = BalanceIdentifier::PenalizedAccount(
                                    initiator_addr
                                        .account_hash()
                                        .ok_or(BlockExecutionError::InvalidTransactionVariant)?,
                                );
                                Ok(InitialBalanceIdentifierResult::IdentifierAndStateError(
                                    penalized_bi,
                                    state_err,
                                ))
                            }
                        }
                    }
                }
                BalanceIdentifierResolution::CheckEvmAccount(_initiator_bi) => {
                    let signer = match ctx.evm_signer() {
                        Some(Ok(signer)) => signer,
                        Some(Err(evm_err)) => {
                            trace!(%transaction_hash, "failed to resolve evm transaction identifier");
                            let penalized_bi = BalanceIdentifier::PenalizedAccount(
                                initiator_addr
                                    .account_hash()
                                    .ok_or(BlockExecutionError::InvalidTransactionVariant)?,
                            );
                            return Ok(InitialBalanceIdentifierResult::IdentifierAndEvmError(
                                penalized_bi,
                                evm_err,
                            ));
                        }
                        None => {
                            trace!(%transaction_hash, "failed to resolve evm transaction identifier");
                            return Err(BlockExecutionError::TransactionConversion(
                                "non evm transaction flagged as evm".to_string(),
                            ));
                        }
                    };

                    let evm_address = match ctx.evm_address() {
                        Some(addr) => addr,
                        None => {
                            trace!(%transaction_hash, "failed to resolve evm transaction identifier");
                            return Err(BlockExecutionError::TransactionConversion(
                                "evm transaction missing its address".to_string(),
                            ));
                        }
                    };

                    match resolve_evm_origin(
                        state_provider,
                        state_root_hash,
                        protocol_version,
                        signer,
                        evm_address,
                    ) {
                        Ok(resolution) => {
                            let bi = resolution.balance_identifier();
                            Ok(InitialBalanceIdentifierResult::IdentifierAndEvmResolution(
                                bi, resolution,
                            ))
                        }
                        Err(err) => {
                            trace!(%transaction_hash, "failed to resolve evm origin");
                            Err(err)
                        }
                    }
                }
            }
        }
        Err(err) => {
            trace!(%transaction_hash, "failed to determine payer_balance_identifier");
            error!(
                %transaction_hash,
                ?err,
                "failed to determine payer_balance_identifier"
            );
            Ok(InitialBalanceIdentifierResult::Unknown)
        }
    }
}
/// Resolves the payer and any deferred identity write for a signed EVM transaction.
///
/// This function only reads state. That matters because it runs before payment
/// preconditions are known to pass. If execution is later allowed, the returned
/// [`EvmIdentityInstruction`] is applied in the tracking copy used for EVM execution.
fn resolve_evm_origin(
    scratch_state: &ScratchGlobalState,
    state_root_hash: Digest,
    protocol_version: ProtocolVersion,
    signer: &PublicKey,
    address: EvmAddress,
) -> Result<EvmOriginResolution, BlockExecutionError> {
    // The signer gives us a Casper `AccountHash` preimage from the secp256k1
    // public key. That account hash is not derivable from the 20-byte EVM
    // address alone, so identity linking must happen while the signed
    // transaction is available.
    // let signer = transaction
    //     .signer()
    //     .map_err(|error| BlockExecutionError::TransactionConversion(error.to_string()))?;
    let account_hash = signer.to_account_hash();
    // Native EVM identities use a deterministic purse derived from the EVM
    // address. Linked Casper accounts use the account's existing main purse
    // instead, so the same key pair can spend the same funds from Casper and
    // Ethereum-style transaction paths.
    //let address = transaction.from();
    let deterministic_purse = casper_types::evm::deterministic_purse(address);
    let mut tracking_copy = match scratch_state.tracking_copy(state_root_hash) {
        Ok(Some(tc)) => tc,
        Ok(None) => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
        Err(gse) => return Err(BlockExecutionError::Lmdb(gse)),
    };

    // `EvmAddr::Account` is now only an identity pointer. It is either
    // `Key::Account` for a linked Casper account or `Key::URef` for an
    // EVM-native purse identity.
    let identity_key = Key::Evm(casper_types::EvmAddr::Account(address));
    match tracking_copy
        .read(&identity_key)
        .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?
    {
        Some(StoredValue::CLValue(cl_value)) => {
            let key = cl_value
                .into_t::<Key>()
                .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
            match key {
                // Existing bridge records are authoritative. Once an EVM
                // address is linked, the payer is the linked Casper account's
                // main purse.
                Key::Account(account_hash) => Ok(EvmOriginResolution::new(
                    BalanceIdentifier::Account(account_hash),
                    EvmIdentityInstruction::None,
                )),
                // Existing EVM-native identities keep paying from their stored
                // purse. We may still plan an upgrade to a Casper link, but
                // only when doing so cannot steal a contract identity or move
                // balances between distinct purses.
                Key::URef(purse) => {
                    let eve_identity_instruction = resolve_evm_native_identity_plan(
                        &mut tracking_copy,
                        protocol_version,
                        address,
                        account_hash,
                        purse,
                        deterministic_purse,
                    )?;
                    Ok(EvmOriginResolution::new(
                        BalanceIdentifier::Purse(purse),
                        eve_identity_instruction,
                    ))
                }
                other => Err(BlockExecutionError::PaymentError(format!(
                    "invalid EVM account identity key: {other}"
                ))),
            }
        }
        Some(stored_value) => Err(BlockExecutionError::PaymentError(format!(
            "unexpected stored value for {identity_key}: expected StoredValue::CLValue(Key), found {}",
            stored_value.type_name()
        ))),
        None => {
            // No identity pointer plus non-empty EVM code means this address is
            // already a contract/runtime-created EVM account. Contracts do not
            // have a signing key, so they must remain EVM-native.
            if evm_account_has_code(&mut tracking_copy, address)? {
                return Ok(EvmOriginResolution::new(
                    BalanceIdentifier::Purse(deterministic_purse),
                    EvmIdentityInstruction::None,
                ));
            }
            match account_main_purse(&mut tracking_copy, protocol_version, account_hash)? {
                // A Casper account exists for the recovered signer, but the EVM
                // address has not been seen before. Use the account for payment
                // immediately and write the bridge only if execution proceeds.
                Some(_) => Ok(EvmOriginResolution::new(
                    BalanceIdentifier::Account(account_hash),
                    EvmIdentityInstruction::LinkExisting {
                        address,
                        account_hash,
                    },
                )),
                // First use of this signing pair on both sides. Runtime will
                // create a Casper account whose main purse is the deterministic
                // EVM purse, then write the bridge record.
                None => Ok(EvmOriginResolution::new(
                    BalanceIdentifier::Purse(deterministic_purse),
                    EvmIdentityInstruction::CreateAccount {
                        address,
                        account_hash,
                        main_purse: deterministic_purse,
                    },
                )),
            }
        }
    }
}

fn resolve_evm_native_identity_plan<R>(
    tracking_copy: &mut TrackingCopy<R>,
    protocol_version: ProtocolVersion,
    address: EvmAddress,
    account_hash: AccountHash,
    purse: casper_types::URef,
    deterministic_purse: casper_types::URef,
) -> Result<EvmIdentityInstruction, BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    // Do not overwrite contract identities, and do not turn an arbitrary purse
    // identity into a Casper account link. The only EVM-native identity that is
    // safe to link is the deterministic purse for this address.
    if evm_account_has_code(tracking_copy, address)? || purse.addr() != deterministic_purse.addr() {
        return Ok(EvmIdentityInstruction::None);
    }

    match account_main_purse(tracking_copy, protocol_version, account_hash)? {
        // If the recovered Casper account already uses the same deterministic
        // purse, replacing the pointer with `Key::Account` preserves the balance
        // location and lets Casper-native flows see the account identity.
        Some(main_purse) if main_purse.addr() == purse.addr() => {
            Ok(EvmIdentityInstruction::LinkExisting {
                address,
                account_hash,
            })
        }
        // A Casper account exists, but its main purse differs from the existing
        // EVM-native purse. Keep the EVM-native identity to avoid moving funds
        // or changing ownership semantics behind the user's back.
        Some(_) => Ok(EvmIdentityInstruction::None),
        // No Casper account exists yet, so creating one backed by the existing
        // deterministic purse preserves balances while giving the signer a
        // Casper account identity.
        None => Ok(EvmIdentityInstruction::CreateAccount {
            address,
            account_hash,
            main_purse: purse,
        }),
    }
}

fn evm_account_has_code<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: EvmAddress,
) -> Result<bool, BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    // Code hash is the cheap contract/EOA discriminator for an EVM address. A
    // non-empty code hash means the address is not a user-controlled signing
    // identity, so runtime must not create or link a Casper account for it.
    let key = Key::Evm(casper_types::EvmAddr::CodeHash(address));
    match tracking_copy
        .read(&key)
        .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?
    {
        Some(StoredValue::CLValue(cl_value)) => {
            let code_hash = cl_value
                .into_t::<casper_types::evm::Hash>()
                .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
            Ok(code_hash != casper_types::evm::EMPTY_CODE_HASH)
        }
        Some(stored_value) => Err(BlockExecutionError::PaymentError(format!(
            "unexpected stored value for {key}: expected StoredValue::CLValue(evm::Hash), found {}",
            stored_value.type_name()
        ))),
        None => Ok(false),
    }
}

//
// fn evm_account_has_nonce<R>(
//     tracking_copy: &mut TrackingCopy<R>,
//     address: EvmAddress,
// ) -> Result<bool, BlockExecutionError>
// where
//     R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
// {
//     let key = Key::Evm(casper_types::EvmAddr::Nonce(address));
//     match tracking_copy
//         .read(&key)
//         .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?
//     {
//         Some(StoredValue::CLValue(cl_value)) => {
//             let _nonce = cl_value
//                 .into_t::<u64>()
//                 .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
//             Ok(true)
//         }
//         Some(stored_value) => Err(BlockExecutionError::PaymentError(format!(
//             "unexpected stored value for {key}: expected StoredValue::CLValue(u64), found {}",
//             stored_value.type_name()
//         ))),
//         None => Ok(false),
//     }
// }

//
// fn apply_evm_proposer_identity<R>(
//     tracking_copy: &mut TrackingCopy<R>,
//     protocol_version: ProtocolVersion,
//     proposer: &PublicKey,
// ) -> Result<(), BlockExecutionError>
// where
//     R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
// {
//     let address = EvmAddress::from_block_proposer_public_key(proposer);
//     let account_hash = proposer.to_account_hash();
//     if account_main_purse(tracking_copy, protocol_version, account_hash)?.is_none() {
//         return Ok(());
//     }
//
//     let identity_key = Key::Evm(casper_types::EvmAddr::Account(address));
//     match tracking_copy
//         .read(&identity_key)
//         .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?
//     {
//         Some(StoredValue::CLValue(cl_value)) => {
//             let identity = cl_value
//                 .into_t::<Key>()
//                 .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
//             match identity {
//                 Key::Account(_) | Key::URef(_) => Ok(()),
//                 other => Err(BlockExecutionError::PaymentError(format!(
//                     "invalid EVM account identity key: {other}"
//                 ))),
//             }
//         }
//         Some(stored_value) => Err(BlockExecutionError::PaymentError(format!(
//             "unexpected stored value for {identity_key}: expected StoredValue::CLValue(Key),
// found {}",             stored_value.type_name()
//         ))),
//         None => {
//             if evm_account_has_code(tracking_copy, address)?
//                 || evm_account_has_nonce(tracking_copy, address)?
//             {
//                 return Ok(());
//             }
//             write_evm_identity(tracking_copy, address, Key::Account(account_hash))
//         }
//     }
// }
//
// fn apply_evm_identity_plan<R>(
//     tracking_copy: &mut TrackingCopy<R>,
//     protocol_version: ProtocolVersion,
//     plan: EvmIdentityInstruction,
// ) -> Result<(), BlockExecutionError>
// where
//     R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
// {
//     // Identity writes are intentionally delayed until after payment
//     // preconditions pass. They are applied to the same tracking copy as EVM
//     // execution so the identity record and nonce/code/storage updates commit or
//     // discard together.
//     match plan {
//         EvmIdentityInstruction::None => Ok(()),
//         EvmIdentityInstruction::LinkExisting {
//             address,
//             account_hash,
//         } => write_evm_identity(tracking_copy, address, Key::Account(account_hash)),
//         EvmIdentityInstruction::CreateAccount {
//             address,
//             account_hash,
//             main_purse,
//         } => {
//             // Another transaction in the same block may have already created
//             // the account through this scratch state. Avoid recreating it, but
//             // still write the EVM identity pointer below.
//             if account_main_purse(tracking_copy, protocol_version, account_hash)?.is_none() {
//                 let account = Account::create(account_hash, NamedKeys::new(), main_purse);
//                 tracking_copy
//                     .create_addressable_entity_from_account(account, protocol_version)
//                     .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
//             }
//             write_evm_identity(tracking_copy, address, Key::Account(account_hash))
//         }
//     }
// }
//
// fn write_evm_identity<R>(
//     tracking_copy: &mut TrackingCopy<R>,
//     address: EvmAddress,
//     identity: Key,
// ) -> Result<(), BlockExecutionError>
// where
//     R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
// {
//     // Keep the bridge record minimal: a CLValue containing the identity `Key`.
//     // Nonce, code hash, bytecode, and storage live under their own EVM keys.
//     let key = Key::Evm(casper_types::EvmAddr::Account(address));
//     let cl_value = CLValue::from_t(identity)
//         .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
//     tracking_copy.write(key, StoredValue::CLValue(cl_value));
//     Ok(())
// }
