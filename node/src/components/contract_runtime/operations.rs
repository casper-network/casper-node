use crate::types::transaction::WasmV2Result;
use casper_executor_wasm::ExecutorV2;
use std::{collections::BTreeMap, convert::TryInto, sync::Arc};
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{
    BlockInfo, ExecutionEngineV1, WasmV1Request, WasmV1Result,
};
use casper_executor_evm::{
    BlockContext as EvmV1Request, BlockHashProvider as EvmBlockHashProvider,
    CallRequest as EvmExecutorCallRequest, CallValidation as EvmCallValidation, EvmExecutor,
    ExecuteKind as EvmExecuteKind, ExecuteRequest as EvmExecuteRequest,
};
use casper_storage::{
    data_access_layer::{
        AuctionMethod, BalanceHoldKind, BalanceIdentifier, BalanceResult, BlockGlobalResult,
        BlockRewardsResult, DataAccessLayer, EntryPointRequest, EntryPointResult,
        EraValidatorsResult, FeeResult, FlushRequest, PruneResult, StepResult, TransferRequest,
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
    BlockHash, BlockHeader, CLValue, Chainspec, ChecksumRegistry, Digest, EntityAddr,
    EvmTransactionError, Gas, HashAddr, InvalidTransaction, InvalidTransactionV1, Key,
    ProtocolVersion, PublicKey, StoredValue, TimeDiff, Transaction, TransactionEntryPoint, U512,
};

use super::{
    types::{SpeculativeExecutionResult, StepOutcome},
    utils::{self, calculate_prune_eras},
    BlockAndExecutionArtifacts, BlockExecutionError, ExecutionPreState, Metrics, StateResultError,
    APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
};
use crate::{
    contract_runtime::types::{
        BalanceIdentifierResolution, EvmOriginResolution, ExecuteBlockContext,
        ExecuteBlockContextError, ExecuteBlockOutcome, InitialBalanceIdentifierResult,
        ProcessRequest, StaticEvmBlockHashProvider, TransactionProcessContext,
    },
    types::{Chunkable, ExecutableBlock, MetaTransaction},
};

// fn process_request(txn_process_ctx: &TransactionProcessContext) -> ProcessRequest {
//     if !txn_process_ctx.allow_execution() {
//         let is_evm = txn_process_ctx.is_evm();
//         if is_evm {
//             let effective_gas_price = txn_process_ctx.evm_effective_gas_price().unwrap_or(0u128);
//             return ProcessRequest::NoExecEvm {
//                 effective_gas_price,
//             };
//         }
//         return ProcessRequest::NoExec;
//     }
//
//     let lane = txn_process_ctx.transaction_lane();
//     if lane == MINT_LANE_ID {
//         return ProcessRequest::NativeMint {
//             session_args: txn_process_ctx.session_args(),
//             entry_point: txn_process_ctx.entry_point(),
//         };
//     }
//     if lane == AUCTION_LANE_ID {
//         return ProcessRequest::NativeAuction {
//             session_args: txn_process_ctx.session_args(),
//             entry_point: txn_process_ctx.entry_point(),
//         };
//     }
//     if txn_process_ctx.is_v1_wasm() {
//         return ProcessRequest::WasmV1 {
//             session_input_data: txn_process_ctx.to_session_input_data(),
//         };
//     }
//     if txn_process_ctx.is_v2_wasm() {
//         return ProcessRequest::WasmV2 {
//             transaction_input: txn_process_ctx.to_transaction_info(),
//         };
//     }
//     match txn_process_ctx.as_evm() {
//         Some(evm_txn) => {
//             let effective_gas_price = match txn_process_ctx.evm_effective_gas_price() {
//                 Some(effective_gas_price) => effective_gas_price,
//                 None => return ProcessRequest::Unknown,
//             };
//
//             let block_gas_limit = txn_process_ctx.evm_block_gas_limit();
//             let base_fee_wei = txn_process_ctx.evm_base_fee_wei();
//
//             ProcessRequest::EvmV1 {
//                 evm_txn: evm_txn.clone(),
//                 base_fee_wei,
//                 effective_gas_price,
//                 block_gas_limit,
//             }
//         }
//         None => ProcessRequest::Unknown,
//     }
// }

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
        current_gas_price,
        next_era_gas_price,
        last_switch_block_hash,
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

    // NOTE this must occur prior to any block processing as subsequent logic
    // will refer to the values being written to GS.
    match data_access_layer.block_global(exec_ctx.block_global_request()) {
        BlockGlobalResult::RootNotFound => {
            return Err(BlockExecutionError::RootNotFound(
                exec_ctx.state_root_hash(),
            ));
        }
        BlockGlobalResult::Failure(err) => {
            return Err(BlockExecutionError::BlockGlobal(format!("{:?}", err)));
        }
        BlockGlobalResult::Success {
            post_state_hash, ..
        } => {
            exec_ctx.with_state_root_hash(post_state_hash);
        }
    }

    // pre-processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_pre_processing
            .observe(exec_ctx.pre_process_elapsed());
    }

    // scratch_state must be used for all processing and post-processing data
    // from here on out, until the effects are applied at the end.
    let scratch_state = data_access_layer.get_scratch_global_state();

    exec_ctx.process_starting();

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
            exec_ctx.with_artifact(txn_process_ctx.into_execution_artifact());
            continue;
        }

        let balance_identifier = {
            let ret = txn_initial_balance_identifier(&scratch_state, &exec_ctx, &txn_process_ctx)
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
                    .map_err(|_| exec_ctx.root_not_found())?;
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
                exec_ctx.with_artifact(txn_process_ctx.into_execution_artifact());
                continue;
            }
        };

        txn_process_ctx.with_initial_balance_identifier(balance_identifier.clone());

        // INITIAL BALANCE
        {
            let initial_balance_result =
                scratch_state.balance(exec_ctx.balance_request(balance_identifier.clone()));

            if let BalanceResult::RootNotFound = initial_balance_result {
                return Err(BlockExecutionError::RootNotFound(
                    exec_ctx.state_root_hash(),
                ));
            }
            txn_process_ctx.with_initial_balance_result(initial_balance_result);
        }

        // CHECK FOR MINIMUM BALANCE
        if !txn_process_ctx.has_sufficient_minimum() {
            // the purse does not have enough to cover the minimum, just record it and move on.
            info!(
                %transaction_hash,
                "Has less than minimum balance"
            );
            exec_ctx.with_artifact(txn_process_ctx.into_execution_artifact());
            continue;
        }

        // REGISTER EXEC ATTEMPT (from this point onward we charge for success and failure)
        txn_process_ctx.with_exec_attempt();

        // PROCESS TRANSACTION
        //let process_request = process_request(&txn_process_ctx);
        let process_request = txn_process_ctx.process_request();
        trace!(%transaction_hash, %process_request, "process_request created");

        // PLACE PROCESSING HOLD TO PREVENT DOUBLE SPEND, IF REQUIRED
        let requires_hold = process_request.requires_processing_hold();
        if requires_hold {
            let hold_amount = txn_process_ctx.cost_to_use();
            let hold_result = scratch_state.balance_hold(
                exec_ctx.balance_hold_request(balance_identifier.clone(), hold_amount),
            );

            exec_ctx.with_state_root_hash(
                scratch_state
                    .commit_effects(exec_ctx.state_root_hash(), hold_result.effects().clone())
                    .map_err(BlockExecutionError::Lmdb)?,
            );

            txn_process_ctx
                .with_balance_hold_result(&hold_result)
                .map_err(|_| exec_ctx.root_not_found())?;
        }

        // PROCESS TRANSACTION
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
                let runtime_args = session_args
                    .as_named()
                    .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
                if let TransactionEntryPoint::Transfer = entry_point {
                    let transfer_request =
                        exec_ctx.transfer_request(&txn_process_ctx, runtime_args.clone());
                    let transfer_result = scratch_state.transfer(transfer_request);
                    exec_ctx.with_state_root_hash(
                        scratch_state
                            .commit_effects(
                                exec_ctx.state_root_hash(),
                                transfer_result.effects().clone(),
                            )
                            .map_err(BlockExecutionError::Lmdb)?,
                    );
                    txn_process_ctx
                        .with_gas_limit_consumed()
                        .with_transfer_result(transfer_result)
                        .map_err(|_| exec_ctx.root_not_found())?;
                } else if let TransactionEntryPoint::Burn = entry_point {
                    let burn_request =
                        exec_ctx.burn_request(&txn_process_ctx, runtime_args.clone());
                    let burn_result = scratch_state.burn(burn_request);
                    exec_ctx.with_state_root_hash(
                        scratch_state
                            .commit_effects(
                                exec_ctx.state_root_hash(),
                                burn_result.effects().clone(),
                            )
                            .map_err(BlockExecutionError::Lmdb)?,
                    );
                    txn_process_ctx
                        .with_gas_limit_consumed()
                        .with_burn_result(burn_result)
                        .map_err(|_| exec_ctx.root_not_found())?;
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
                let runtime_args = session_args
                    .as_named()
                    .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
                match AuctionMethod::from_parts(entry_point.clone(), runtime_args, chainspec) {
                    Ok(auction_method) => {
                        let bidding_result = scratch_state
                            .bidding(exec_ctx.bidding_request(&txn_process_ctx, auction_method));
                        exec_ctx.with_state_root_hash(
                            scratch_state
                                .commit_effects(
                                    exec_ctx.state_root_hash(),
                                    bidding_result.effects().clone(),
                                )
                                .map_err(BlockExecutionError::Lmdb)?,
                        );
                        txn_process_ctx
                            .with_gas_limit_consumed()
                            .with_bidding_result(bidding_result)
                            .map_err(|_| exec_ctx.root_not_found())?;
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
                exec_ctx.wasm_v1_starting();
                match exec_ctx.wasm_v1_session_request(&txn_process_ctx, &session_input_data) {
                    Ok(wasm_v1_request) => {
                        trace!(%transaction_hash, ?lane_id, ?wasm_v1_request, "able to get wasm v1 request");
                        let wasm_v1_result =
                            execution_engine_v1.execute(&scratch_state, wasm_v1_request);
                        trace!(%transaction_hash, ?lane_id, ?wasm_v1_result, "able to get wasm v1 result");
                        exec_ctx.with_state_root_hash(
                            scratch_state
                                .commit_effects(
                                    exec_ctx.state_root_hash(),
                                    wasm_v1_result.effects().clone(),
                                )
                                .map_err(BlockExecutionError::Lmdb)?,
                        );
                        // note: consumed is scraped from wasm_v1_result along w/ other fields
                        txn_process_ctx
                            .with_wasm_v1_result(wasm_v1_result)
                            .map_err(|_| exec_ctx.root_not_found())?;
                    }
                    Err(ire) => {
                        debug!(%transaction_hash, ?lane_id, ?ire, "unable to get wasm v1 request");
                        txn_process_ctx.with_invalid_wasm_v1_request(&ire);
                    }
                };
                if let Some(metrics) = metrics.as_ref() {
                    metrics.exec_wasm_v1.observe(exec_ctx.wasm_v1_elapsed());
                }
            }
            ProcessRequest::WasmV2 { transaction_input } => {
                exec_ctx.wasm_v2_starting();
                match exec_ctx.wasm_v2_request(&txn_process_ctx, transaction_input.clone()) {
                    Ok(wasm_v2_request) => {
                        let pre_root = exec_ctx.state_root_hash();
                        match wasm_v2_request.execute(
                            &execution_engine_v2,
                            pre_root,
                            &scratch_state,
                        ) {
                            Ok(wasm_v2_result) => {
                                match &wasm_v2_result {
                                    WasmV2Result::Install(install_result) => {
                                        info!(
                                                contract_hash=base16::encode_lower(&install_result.smart_contract_addr()),
                                                pre_state_root_hash=%pre_root,
                                                post_state_root_hash=%install_result.post_state_hash(),
                                                "install contract result");
                                    }

                                    WasmV2Result::Execute(execute_result) => {
                                        info!(
                                                pre_state_root_hash=%pre_root,
                                                post_state_root_hash=%execute_result.post_state_hash(),
                                                host_error=?execute_result.host_error.as_ref(),
                                                "execute contract result");
                                    }
                                }

                                exec_ctx.with_state_root_hash(wasm_v2_result.post_state_hash());
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
                    metrics.exec_wasm_v2.observe(exec_ctx.wasm_v2_elapsed());
                }
            }
            ProcessRequest::EvmV1 {
                evm_txn,
                base_fee_wei,
                effective_gas_price,
                block_gas_limit,
            } => {
                exec_ctx.evm_v1_starting();

                let pre_hash = exec_ctx.state_root_hash();
                let mut tracking_copy = scratch_state
                    .tracking_copy(pre_hash)
                    .map_err(BlockExecutionError::Lmdb)?
                    .ok_or(BlockExecutionError::RootNotFound(pre_hash))?;

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
                // apply_evm_proposer_identity(&mut tracking_copy, protocol_version,
                // &proposer)?;

                let evm_v1_request =
                    exec_ctx.evm_v1_request(evm_txn.clone(), block_gas_limit, base_fee_wei);
                let outcome = EvmExecutor::new(chainspec.evm_config)
                    .execute(&mut tracking_copy, evm_v1_request)
                    .map_err(|error| {
                        BlockExecutionError::TransactionConversion(error.to_string())
                    })?;
                let execution_effects = tracking_copy.effects();
                exec_ctx.with_state_root_hash(
                    scratch_state
                        .commit_effects(pre_hash, execution_effects.clone())
                        .map_err(BlockExecutionError::Lmdb)?,
                );

                txn_process_ctx.with_evm_execution_outcome(
                    outcome,
                    effective_gas_price,
                    execution_effects,
                );
                if let Some(metrics) = metrics.as_ref() {
                    metrics.exec_evm_v1.observe(exec_ctx.evm_v1_elapsed());
                }
            }
            ProcessRequest::Unknown => {
                // this should be unreachable
                unreachable!("Unknown transaction execution target");
            }
        }

        // CLEAR ALL EXPIRED BALANCE HOLDS
        {
            let hold_request = exec_ctx
                .clear_balance_hold_request(BalanceHoldKind::All, balance_identifier.clone());
            let hold_result = scratch_state.balance_hold(hold_request);
            exec_ctx.with_state_root_hash(
                scratch_state
                    .commit_effects(exec_ctx.state_root_hash(), hold_result.effects().clone())
                    .map_err(BlockExecutionError::Lmdb)?,
            );
            txn_process_ctx
                .with_balance_hold_result(&hold_result)
                .map_err(|_| exec_ctx.root_not_found())?;
        }

        // HANDLE REFUND (IF ANY)
        match exec_ctx.refund_mode(&txn_process_ctx) {
            None => {
                txn_process_ctx.with_refund_amount(U512::zero());
            }
            Some(refund_mode) => {
                let handle_refund_request =
                    exec_ctx.handle_refund_request(&txn_process_ctx, refund_mode);
                let handle_refund_result = scratch_state.handle_refund(handle_refund_request);
                let refunded_amount = handle_refund_result.refund_amount();
                exec_ctx.with_state_root_hash(
                    scratch_state
                        .commit_effects(
                            exec_ctx.state_root_hash(),
                            handle_refund_result.effects().clone(),
                        )
                        .map_err(BlockExecutionError::Lmdb)?,
                );
                txn_process_ctx
                    .with_handle_refund_result(&handle_refund_result)
                    .map_err(|_| exec_ctx.root_not_found())?;

                txn_process_ctx.with_refund_amount(refunded_amount);
            }
        }

        // HANDLE FEE (IF ANY)
        match exec_ctx.fee_mode(&txn_process_ctx) {
            None => {}
            Some(fee_mode) => {
                if fee_mode.requires_hold() {
                    let hold_request =
                        exec_ctx.gas_hold_request(balance_identifier, txn_process_ctx.fee_amount());
                    let hold_result = scratch_state.balance_hold(hold_request);
                    exec_ctx.with_state_root_hash(
                        scratch_state
                            .commit_effects(
                                exec_ctx.state_root_hash(),
                                hold_result.effects().clone(),
                            )
                            .map_err(BlockExecutionError::Lmdb)?,
                    );
                    txn_process_ctx
                        .with_balance_hold_result(&hold_result)
                        .map_err(|_| exec_ctx.root_not_found())?;
                }
                let handle_fee_request = exec_ctx.handle_fee_request(&txn_process_ctx, fee_mode);
                let handle_fee_result = scratch_state.handle_fee(handle_fee_request);
                exec_ctx.with_state_root_hash(
                    scratch_state
                        .commit_effects(
                            exec_ctx.state_root_hash(),
                            handle_fee_result.effects().clone(),
                        )
                        .map_err(BlockExecutionError::Lmdb)?,
                );
                txn_process_ctx
                    .with_handle_fee_result(&handle_fee_result)
                    .map_err(|_| exec_ctx.root_not_found())?;
            }
        };

        if let Some(err_msg) = txn_process_ctx.error_message() {
            debug!(%transaction_hash, ?err_msg, "transaction error");
        }

        exec_ctx.with_artifact(txn_process_ctx.into_execution_artifact());
    }

    // transaction processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_tnx_processing
            .observe(exec_ctx.process_elapsed());
    }

    exec_ctx.post_process_starting();

    // REGISTER CHECKSUMS
    {
        // the canonical full set of approvals and metadata must be historically verifiable.
        // to allow this, we must calculate and store checksums for approvals and execution effects
        //   across all transactions in the block.
        // block synchronization uses these checksums to ensure correct complete block data.
        // TODO: shift this to an iterator if possible
        let artifacts = exec_ctx.execution_artifacts();
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
            .commit_effects(exec_ctx.state_root_hash(), effects)
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
        exec_ctx.block_rewards_starting();

        // Pay out block fees, if relevant. This auto-commits
        {
            let fee_request = exec_ctx.block_fee_request();
            debug!(?fee_request, "distributing block fees");
            match scratch_state.distribute_fees(fee_request) {
                FeeResult::RootNotFound => {
                    return Err(exec_ctx.root_not_found());
                }
                FeeResult::Failure(fer) => return Err(BlockExecutionError::DistributeFees(fer)),
                FeeResult::Success {
                    post_state_hash, ..
                } => {
                    debug!("fee distribution success");
                    exec_ctx.with_state_root_hash(post_state_hash);
                }
            }
        }

        // Pay out block rewards, if relevant. This auto-commits
        {
            let rewards_request = exec_ctx.block_rewards_request(rewards.clone());
            debug!(?rewards_request, "distributing block rewards");
            match scratch_state.distribute_block_rewards(rewards_request) {
                BlockRewardsResult::RootNotFound => {
                    return Err(exec_ctx.root_not_found());
                }
                BlockRewardsResult::Failure(bre) => {
                    return Err(BlockExecutionError::DistributeBlockRewards(bre));
                }
                BlockRewardsResult::Success {
                    post_state_hash, ..
                } => {
                    debug!("rewards distribution success");
                    exec_ctx.with_state_root_hash(post_state_hash);
                }
            }
        }

        if let Some(metrics) = metrics.as_ref() {
            metrics
                .block_rewards_payout
                .observe(exec_ctx.block_rewards_elapsed());
        }
    }

    // if era report is some, this is a switch block. a series of end-of-era extra processing must
    // transpire before this block is entirely finished.
    if let Some(era_report) = &executable_block.era_report {
        // step processing starts now
        exec_ctx.step_starting();

        let step_request = exec_ctx.step_request(era_report);

        debug!("committing step");
        let step_result = scratch_state.step(step_request);
        debug_assert!(step_result.is_success(), "{:?}", step_result);
        trace!(?step_result, "step response");

        let step_effects = match step_result {
            StepResult::RootNotFound => {
                return Err(exec_ctx.root_not_found());
            }
            StepResult::Failure(err) => return Err(BlockExecutionError::Step(err)),
            StepResult::Success {
                effects,
                post_state_hash,
                ..
            } => {
                exec_ctx.with_state_root_hash(post_state_hash);
                effects
            }
        };
        debug!("step committed");

        if let Some(metrics) = metrics.as_ref() {
            let elapsed = exec_ctx.step_elapsed();
            metrics.commit_step.observe(elapsed);
            metrics.latest_commit_step.set(elapsed);
        }

        let era_validators_request = exec_ctx.era_validators_request();

        let upcoming_era_validators = match data_access_layer.era_validators(era_validators_request)
        {
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
                .observe(exec_ctx.step_elapsed());
        }

        exec_ctx.with_step_outcome(StepOutcome {
            step_effects,
            upcoming_era_validators,
        });
    }

    // Pruning -- this is orthogonal to the contents of the block, but we deliberately do it
    // at the end to avoid a read ordering issue during block execution.
    if let Some(previous_block_height) = exec_ctx.prev_block_height() {
        let activation_point_era_id = exec_ctx.activation_point_era_id();
        let prune_batch_size = exec_ctx.prune_batch_size();

        if let Some(keys_to_prune) = calculate_prune_eras(
            activation_point_era_id,
            key_block_height_for_activation_point,
            previous_block_height,
            prune_batch_size,
        ) {
            exec_ctx.prune_starting();

            let first_key = keys_to_prune.first().copied();
            let last_key = keys_to_prune.last().copied();
            let prune_root = exec_ctx.state_root_hash();
            info!(
                previous_block_height,
                %key_block_height_for_activation_point,
                %prune_root,
                first_key=?first_key,
                last_key=?last_key,
                "commit prune: preparing prune config"
            );
            let prune_request = exec_ctx.prune_request(keys_to_prune);
            match scratch_state.prune(prune_request) {
                PruneResult::RootNotFound => {
                    error!(
                        previous_block_height,
                        %prune_root,
                        "commit prune: root not found"
                    );
                    return Err(exec_ctx.root_not_found());
                }
                PruneResult::Failure(tce) => {
                    error!(?tce, "commit prune: failure");
                    return Err(tce.into());
                }
                PruneResult::MissingKey => {
                    warn!(
                        previous_block_height,
                        %prune_root,
                        "commit prune: key does not exist"
                    );
                }
                PruneResult::Success {
                    post_state_hash, ..
                } => {
                    info!(
                        previous_block_height,
                        %key_block_height_for_activation_point,
                        %prune_root,
                        %post_state_hash,
                        first_key=?first_key,
                        last_key=?last_key,
                        "commit prune: success"
                    );
                    exec_ctx.with_state_root_hash(post_state_hash);
                }
            }
            if let Some(metrics) = metrics.as_ref() {
                metrics.pruning_time.observe(exec_ctx.prune_elapsed());
            }
        }
    }

    {
        // Finally, the new state-root-hash from the cumulative changes to global state is
        // returned when they are written to LMDB.
        exec_ctx.db_write_starting();
        exec_ctx.with_state_root_hash(
            data_access_layer
                .write_scratch_to_db(exec_ctx.state_root_hash(), scratch_state)
                .map_err(BlockExecutionError::Lmdb)?,
        );
        if let Some(metrics) = metrics.as_ref() {
            metrics
                .scratch_lmdb_write_time
                .observe(exec_ctx.db_write_elapsed());
        }

        // Flush once, after all data mutation.
        exec_ctx.db_flush_starting();
        let flush_result = data_access_layer.flush(FlushRequest::new());
        if let Err(gse) = flush_result.as_error() {
            error!("failed to flush lmdb");
            return Err(BlockExecutionError::Lmdb(gse));
        }
        if let Some(metrics) = metrics.as_ref() {
            metrics
                .database_flush_time
                .observe(exec_ctx.db_flush_elapsed());
        }
    }

    let merkle_proof = match data_access_layer
        .tracking_copy(exec_ctx.state_root_hash())
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
        None => return Err(exec_ctx.root_not_found()),
    };

    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_post_processing
            .observe(exec_ctx.post_process_elapsed());
    }

    let outcome = exec_ctx.into_outcome(merkle_proof);

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block_total.observe(outcome.total_elapsed());
    }

    match outcome {
        ExecuteBlockOutcome::FailedToCreateEraEnd {
            err_msg,
            maybe_era_report,
            maybe_next_era_validator_weights,
            ..
        } => {
            error!("{}", err_msg);
            Err(BlockExecutionError::FailedToCreateEraEnd {
                maybe_era_report,
                maybe_next_era_validator_weights,
            })
        }
        ExecuteBlockOutcome::Success { ret, .. } => Ok(ret),
    }
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
    let block_context = EvmV1Request {
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
    state_provider: &ScratchGlobalState,
    exec_ctx: &ExecuteBlockContext,
    txn_ctx: &TransactionProcessContext,
) -> Result<InitialBalanceIdentifierResult, BlockExecutionError> {
    let state_root_hash = exec_ctx.state_root_hash();
    let protocol_version = exec_ctx.protocol_version();
    let addressable_entity_enabled = exec_ctx.addressable_entity_enabled();

    let transaction_hash = txn_ctx.transaction_hash();
    let initiator_addr = txn_ctx.initiator_addr().clone();

    match txn_ctx.balance_identifier_resolution() {
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
                            txn_ctx.contract_direct_address(),
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
                    let signer = match txn_ctx.evm_signer() {
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

                    let evm_address = match txn_ctx.evm_address() {
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
