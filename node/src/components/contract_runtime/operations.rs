pub(crate) mod wasm_v2_request;

use casper_executor_wasm::ExecutorV2;
use itertools::Itertools;
use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};
use tracing::{debug, error, info, trace, warn};
use wasm_v2_request::WasmV2Request;

use casper_execution_engine::engine_state::{
    BlockInfo, ExecutionEngineV1, WasmV1Request, WasmV1Result,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        balance::BalanceHandling,
        forced_undelegate::{ForcedUndelegateRequest, ForcedUndelegateResult},
        mint::BalanceIdentifierTransferArgs,
        AuctionMethod, BalanceHoldKind, BalanceHoldRequest, BalanceIdentifier,
        BalanceIdentifierPurseRequest, BalanceIdentifierPurseResult, BalanceRequest,
        BiddingRequest, BlockGlobalRequest, BlockGlobalResult, BlockRewardsRequest,
        BlockRewardsResult, DataAccessLayer, EntryPointRequest, EntryPointResult,
        EraValidatorsRequest, EraValidatorsResult, EvictItem, FeeRequest, FeeResult, FlushRequest,
        HandleFeeMode, HandleFeeRequest, HandleRefundMode, HandleRefundRequest,
        InsufficientBalanceHandling, ProofHandling, PruneRequest, PruneResult, StepRequest,
        StepResult, TransferRequest,
    },
    global_state::state::{
        lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, ScratchProvider,
        StateProvider, StateReader,
    },
    system::runtime_native::Config as NativeRuntimeConfig,
};
use casper_types::{
    bytesrepr::{self, ToBytes, U32_SERIALIZED_LENGTH},
    execution::{Effects, ExecutionResult, TransformKindV2, TransformV2},
    system::handle_payment::ARG_AMOUNT,
    BlockHash, BlockHeader, BlockTime, BlockV2, CLValue, Chainspec, ChecksumRegistry, Digest,
    EntityAddr, EraEndV2, EraId, FeeHandling, Gas, InvalidTransaction, InvalidTransactionV1, Key,
    ProtocolVersion, PublicKey, RefundHandling, Transaction, AUCTION_LANE_ID, MINT_LANE_ID, U512,
};

use super::{
    types::{SpeculativeExecutionResult, StepOutcome},
    utils::{self, calculate_prune_eras},
    BlockAndExecutionArtifacts, BlockExecutionError, ExecutionPreState, Metrics, StateResultError,
    APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
};
use crate::{
    components::fetcher::FetchItem,
    contract_runtime::types::ExecutionArtifactBuilder,
    types::{self, Chunkable, ExecutableBlock, InternalEraReport, MetaTransaction},
};

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
    execution_engine_v1: &ExecutionEngineV1,
    execution_engine_v2: ExecutorV2,
    chainspec: &Chainspec,
    metrics: Option<Arc<Metrics>>,
    execution_pre_state: ExecutionPreState,
    executable_block: ExecutableBlock,
    key_block_height_for_activation_point: u64,
    current_gas_price: u8,
    next_era_gas_price: Option<u8>,
    last_switch_block_hash: Option<BlockHash>,
) -> Result<BlockAndExecutionArtifacts, BlockExecutionError> {
    let block_height = executable_block.height;
    if block_height != execution_pre_state.next_block_height() {
        return Err(BlockExecutionError::WrongBlockHeight {
            executable_block: Box::new(executable_block),
            execution_pre_state: Box::new(execution_pre_state),
        });
    }
    if executable_block.era_report.is_some() && next_era_gas_price.is_none() {
        return Err(BlockExecutionError::FailedToGetNewEraGasPrice {
            era_id: executable_block.era_id.successor(),
        });
    }
    let start = Instant::now();
    let protocol_version = chainspec.protocol_version();
    let activation_point_era_id = chainspec.protocol_config.activation_point.era_id();
    let prune_batch_size = chainspec.core_config.prune_batch_size;
    let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);
    let addressable_entity_enabled = chainspec.core_config.enable_addressable_entity();

    if addressable_entity_enabled != data_access_layer.enable_addressable_entity {
        return Err(BlockExecutionError::InvalidAESetting(
            data_access_layer.enable_addressable_entity,
        ));
    }

    // scrape variables from execution pre state
    let parent_hash = execution_pre_state.parent_hash();
    let parent_seed = execution_pre_state.parent_seed();
    let parent_block_hash = execution_pre_state.parent_hash();
    let pre_state_root_hash = execution_pre_state.pre_state_root_hash();
    let mut state_root_hash = pre_state_root_hash; // initial state root is parent's state root

    let payment_balance_addr =
        match data_access_layer.balance_purse(BalanceIdentifierPurseRequest::new(
            state_root_hash,
            protocol_version,
            BalanceIdentifier::Payment,
        )) {
            BalanceIdentifierPurseResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash))
            }
            BalanceIdentifierPurseResult::Failure(tce) => {
                return Err(BlockExecutionError::BlockGlobal(format!("{:?}", tce)));
            }
            BalanceIdentifierPurseResult::Success { purse_addr } => purse_addr,
        };

    // scrape variables from executable block
    let block_time = BlockTime::new(executable_block.timestamp.millis());

    let proposer = executable_block.proposer.clone();
    let era_id = executable_block.era_id;
    let mut artifacts = Vec::with_capacity(executable_block.transactions.len());

    // set up accounting variables / settings
    let insufficient_balance_handling = InsufficientBalanceHandling::HoldRemaining;
    let refund_handling = chainspec.core_config.refund_handling;
    let fee_handling = chainspec.core_config.fee_handling;
    let baseline_motes_amount = chainspec.core_config.baseline_motes_amount_u512();
    let balance_handling = BalanceHandling::Available;

    // get scratch state, which must be used for all processing and post-processing data
    // requirements.
    let scratch_state = data_access_layer.get_scratch_global_state();

    // pre-processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_pre_processing
            .observe(start.elapsed().as_secs_f64());
    }

    // grabbing transaction id's now to avoid cloning transactions
    let transaction_ids = executable_block
        .transactions
        .iter()
        .map(Transaction::fetch_id)
        .collect_vec();

    // transaction processing starts now
    let txn_processing_start = Instant::now();

    // put block_time to global state
    // NOTE this must occur prior to any block processing as subsequent logic
    // will refer to the block time value being written to GS now.
    match scratch_state.block_global(BlockGlobalRequest::block_time(
        state_root_hash,
        protocol_version,
        block_time,
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

    // put protocol version to global state
    match scratch_state.block_global(BlockGlobalRequest::set_protocol_version(
        state_root_hash,
        protocol_version,
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

    // put enable addressable entity flag to global state
    match scratch_state.block_global(BlockGlobalRequest::set_addressable_entity(
        state_root_hash,
        protocol_version,
        addressable_entity_enabled,
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

    let transaction_config = &chainspec.transaction_config;

    for stored_transaction in executable_block.transactions {
        let mut artifact_builder =
            ExecutionArtifactBuilder::new(&stored_transaction, baseline_motes_amount);
        let transaction =
            MetaTransaction::from_transaction(&stored_transaction, transaction_config)
                .map_err(|err| BlockExecutionError::TransactionConversion(err.to_string()))?;
        let initiator_addr = transaction.initiator_addr();
        let transaction_hash = transaction.hash();
        let transaction_args = transaction.session_args().clone();
        let entry_point = transaction.entry_point();
        let authorization_keys = transaction.signers();

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

        the third important value is the amount of computation consumed by executing a
        transaction  for native transactions there is no wasm and the consumed always
        equals the limit  for bytecode / wasm based transactions the consumed is based on
        what opcodes were executed and can range from >=0 to <=gas_limit.
        consumed is determined after execution and is used for refund & fee post-processing.

        we check these top level concerns early so that we can skip if there is an error
        */

        // NOTE: this is the allowed computation limit (gas limit)
        let gas_limit =
            match stored_transaction.gas_limit(chainspec, transaction.transaction_lane()) {
                Ok(gas) => gas,
                Err(ite) => {
                    debug!(%transaction_hash, %ite, "invalid transaction (gas limit)");
                    artifact_builder.with_invalid_transaction(&ite);
                    artifacts.push(artifact_builder.build());
                    continue;
                }
            };
        artifact_builder.with_gas_limit(gas_limit);

        // NOTE: this is the actual adjusted cost that we charge for (gas limit * gas price)
        let cost = match stored_transaction.gas_cost(
            chainspec,
            transaction.transaction_lane(),
            current_gas_price,
        ) {
            Ok(motes) => motes.value(),
            Err(ite) => {
                debug!(%transaction_hash, "invalid transaction (motes conversion)");
                artifact_builder.with_invalid_transaction(&ite);
                artifacts.push(artifact_builder.build());
                continue;
            }
        };
        artifact_builder.with_added_cost(cost);

        let is_standard_payment = transaction.is_standard_payment();
        let is_custom_payment = !is_standard_payment && transaction.is_custom_payment();
        let is_v1_wasm = transaction.is_v1_wasm();
        let is_v2_wasm = transaction.is_v2_wasm();
        let refund_purse_active = is_custom_payment;
        if refund_purse_active {
            // if custom payment before doing any processing, initialize the initiator's main purse
            //  to be the refund purse for this transaction.
            // NOTE: when executed, custom payment logic has the option to call set_refund_purse
            //  on the handle payment contract to set up a different refund purse, if desired.
            let handle_refund_request = HandleRefundRequest::new(
                native_runtime_config.clone(),
                state_root_hash,
                protocol_version,
                transaction_hash,
                HandleRefundMode::SetRefundPurse {
                    target: Box::new(initiator_addr.clone().into()),
                },
            );
            let handle_refund_result = scratch_state.handle_refund(handle_refund_request);
            if let Err(root_not_found) =
                artifact_builder.with_set_refund_purse_result(&handle_refund_result)
            {
                if root_not_found {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                }
                artifacts.push(artifact_builder.build());
                continue; // don't commit effects, move on
            }
            state_root_hash = scratch_state
                .commit_effects(state_root_hash, handle_refund_result.effects().clone())?;
        }

        {
            // Ensure the initiator's main purse can cover the penalty payment before proceeding.
            let initial_balance_result = scratch_state.balance(BalanceRequest::new(
                state_root_hash,
                protocol_version,
                initiator_addr.clone().into(),
                balance_handling,
                ProofHandling::NoProofs,
            ));

            if let Err(root_not_found) = artifact_builder
                .with_initial_balance_result(initial_balance_result.clone(), baseline_motes_amount)
            {
                if root_not_found {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                }
                trace!(%transaction_hash, "insufficient initial balance");
                debug!(%transaction_hash, ?initial_balance_result, %baseline_motes_amount, "insufficient initial balance");
                artifacts.push(artifact_builder.build());
                // only reads have happened so far, and we can't charge due
                // to insufficient balance, so move on with no effects committed
                continue;
            }
        }

        let mut balance_identifier = {
            if is_standard_payment {
                let contract_might_pay =
                    addressable_entity_enabled && transaction.is_contract_by_hash_invocation();

                if contract_might_pay {
                    match invoked_contract_will_pay(&scratch_state, state_root_hash, &transaction) {
                        Ok(Some(entity_addr)) => BalanceIdentifier::Entity(entity_addr),
                        Ok(None) => {
                            // the initiating account pays using its main purse
                            trace!(%transaction_hash, "direct invocation with account payment");
                            initiator_addr.clone().into()
                        }
                        Err(err) => {
                            trace!(%transaction_hash, "failed to resolve contract self payment");
                            artifact_builder
                                .with_state_result_error(err)
                                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                            BalanceIdentifier::PenalizedAccount(
                                initiator_addr.clone().account_hash(),
                            )
                        }
                    }
                } else {
                    // the initiating account pays using its main purse
                    trace!(%transaction_hash, "account session with standard payment");
                    initiator_addr.clone().into()
                }
            } else if is_v2_wasm {
                // vm2 does not support custom payment, so it MUST be standard payment
                // if transaction runtime is v2 then the initiating account will pay using
                // the refund purse
                initiator_addr.clone().into()
            } else if is_custom_payment {
                // this is the custom payment flow
                // the initiating account will pay, but wants to do so with a different purse or
                // in a custom way. If anything goes wrong, penalize the sender, do not execute
                let custom_payment_gas_limit =
                    Gas::new(chainspec.transaction_config.native_transfer_minimum_motes * 5);
                let pay_result = match WasmV1Request::new_custom_payment(
                    BlockInfo::new(
                        state_root_hash,
                        block_time,
                        parent_block_hash,
                        block_height,
                        protocol_version,
                    ),
                    custom_payment_gas_limit,
                    &transaction.to_payment_input_data(),
                ) {
                    Ok(mut pay_request) => {
                        pay_request
                            .args
                            .insert(ARG_AMOUNT, cost)
                            .map_err(|e| BlockExecutionError::PaymentError(e.to_string()))?;
                        execution_engine_v1.execute(&scratch_state, pay_request)
                    }
                    Err(error) => {
                        WasmV1Result::invalid_executable_item(custom_payment_gas_limit, error)
                    }
                };

                let insufficient_payment_deposited =
                    !pay_result.balance_increased_by_amount(payment_balance_addr, cost);

                if insufficient_payment_deposited || pay_result.error().is_some() {
                    // Charge initiator for the penalty payment amount
                    // the most expedient way to do this that aligns with later code
                    // is to transfer from the initiator's main purse to the payment purse
                    let transfer_result = scratch_state.transfer(TransferRequest::new_indirect(
                        native_runtime_config.clone(),
                        state_root_hash,
                        protocol_version,
                        transaction_hash,
                        initiator_addr.clone(),
                        authorization_keys.clone(),
                        BalanceIdentifierTransferArgs::new(
                            None,
                            initiator_addr.clone().into(),
                            BalanceIdentifier::Payment,
                            baseline_motes_amount,
                            None,
                        ),
                    ));

                    let msg = match pay_result.error() {
                        Some(err) => format!("{}", err),
                        None => {
                            if insufficient_payment_deposited {
                                "Insufficient custom payment".to_string()
                            } else {
                                // this should be unreachable due to guard condition above
                                let unk = "Unknown custom payment issue";
                                warn!(%transaction_hash, unk);
                                debug_assert!(false, "{}", unk);
                                unk.to_string()
                            }
                        }
                    };
                    // commit penalty payment effects
                    state_root_hash = scratch_state
                        .commit_effects(state_root_hash, transfer_result.effects().clone())?;
                    artifact_builder
                        .with_error_message(msg)
                        .with_transfer_result(transfer_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    trace!(%transaction_hash, balance_identifier=?BalanceIdentifier::PenalizedPayment, "account session with custom payment failed");
                    BalanceIdentifier::PenalizedPayment
                } else {
                    // commit successful effects
                    state_root_hash = scratch_state
                        .commit_effects(state_root_hash, pay_result.effects().clone())?;
                    artifact_builder
                        .with_wasm_v1_result(pay_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    trace!(%transaction_hash, balance_identifier=?BalanceIdentifier::Payment, "account session with custom payment success");
                    BalanceIdentifier::Payment
                }
            } else {
                BalanceIdentifier::PenalizedAccount(initiator_addr.clone().account_hash())
            }
        };

        let post_payment_balance_result = scratch_state.balance(BalanceRequest::new(
            state_root_hash,
            protocol_version,
            balance_identifier.clone(),
            balance_handling,
            ProofHandling::NoProofs,
        ));

        let lane_id = transaction.transaction_lane();

        let allow_execution = {
            let is_not_penalized = !balance_identifier.is_penalty();
            // in the case of custom payment, we do all payment processing up front after checking
            // if the initiator can cover the penalty payment, and then either charge the full
            // amount in the happy path or the penalty amount in the sad path...in whichever case
            // the sad path is handled by is_penalty and the balance in the payment purse is
            // the penalty payment or the full amount but is 'sufficient' either way
            let is_sufficient_balance =
                is_custom_payment || post_payment_balance_result.is_sufficient(cost);
            let is_allowed_by_chainspec = chainspec.is_supported(lane_id);
            let allow = is_not_penalized && is_sufficient_balance && is_allowed_by_chainspec;
            if !allow {
                if artifact_builder.error_message().is_none() {
                    artifact_builder.with_error_message(format!(
                        "penalized: {}, sufficient balance: {}, allowed by chainspec: {}",
                        !is_not_penalized, is_sufficient_balance, is_allowed_by_chainspec
                    ));
                }
                info!(%transaction_hash, ?balance_identifier, ?is_sufficient_balance, ?is_not_penalized, ?is_allowed_by_chainspec, "payment preprocessing unsuccessful");
            } else {
                debug!(%transaction_hash, ?balance_identifier, ?is_sufficient_balance, ?is_not_penalized, ?is_allowed_by_chainspec, "payment preprocessing successful");
            }
            allow
        };

        if allow_execution {
            debug!(%transaction_hash, ?allow_execution, "execution allowed");
            if is_standard_payment {
                // place a processing hold on the paying account to prevent double spend.
                let hold_amount = cost;
                let hold_request = BalanceHoldRequest::new_processing_hold(
                    state_root_hash,
                    protocol_version,
                    balance_identifier.clone(),
                    hold_amount,
                    insufficient_balance_handling,
                );
                let hold_result = scratch_state.balance_hold(hold_request);
                state_root_hash =
                    scratch_state.commit_effects(state_root_hash, hold_result.effects().clone())?;
                artifact_builder
                    .with_balance_hold_result(&hold_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
            }

            trace!(%transaction_hash, ?lane_id, "eligible for execution");
            match lane_id {
                lane_id if lane_id == MINT_LANE_ID => {
                    let runtime_args = transaction_args
                        .as_named()
                        .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
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
                    let consumed = gas_limit;
                    state_root_hash = scratch_state
                        .commit_effects(state_root_hash, transfer_result.effects().clone())?;
                    artifact_builder
                        .with_added_consumed(consumed)
                        .with_transfer_result(transfer_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                }
                lane_id if lane_id == AUCTION_LANE_ID => {
                    let runtime_args = transaction_args
                        .as_named()
                        .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
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
                            let consumed = gas_limit;
                            state_root_hash = scratch_state.commit_effects(
                                state_root_hash,
                                bidding_result.effects().clone(),
                            )?;
                            artifact_builder
                                .with_added_consumed(consumed)
                                .with_bidding_result(bidding_result)
                                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                        }
                        Err(ame) => {
                            error!(
                                %transaction_hash,
                                ?ame,
                                "failed to determine auction method"
                            );
                            artifact_builder.with_auction_method_error(&ame);
                        }
                    };
                }
                _ if is_v1_wasm => {
                    let wasm_v1_start = Instant::now();
                    let session_input_data = transaction.to_session_input_data();
                    match WasmV1Request::new_session(
                        BlockInfo::new(
                            state_root_hash,
                            block_time,
                            parent_block_hash,
                            block_height,
                            protocol_version,
                        ),
                        gas_limit,
                        &session_input_data,
                    ) {
                        Ok(wasm_v1_request) => {
                            trace!(%transaction_hash, ?lane_id, ?wasm_v1_request, "able to get wasm v1 request");
                            let wasm_v1_result =
                                execution_engine_v1.execute(&scratch_state, wasm_v1_request);
                            trace!(%transaction_hash, ?lane_id, ?wasm_v1_result, "able to get wasm v1 result");
                            state_root_hash = scratch_state.commit_effects(
                                state_root_hash,
                                wasm_v1_result.effects().clone(),
                            )?;
                            // note: consumed is scraped from wasm_v1_result along w/ other fields
                            artifact_builder
                                .with_wasm_v1_result(wasm_v1_result)
                                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                        }
                        Err(ire) => {
                            debug!(%transaction_hash, ?lane_id, ?ire, "unable to get wasm v1 request");
                            artifact_builder.with_invalid_wasm_v1_request(&ire);
                        }
                    };
                    if let Some(metrics) = metrics.as_ref() {
                        metrics
                            .exec_wasm_v1
                            .observe(wasm_v1_start.elapsed().as_secs_f64());
                    }
                }
                _ if is_v2_wasm => match WasmV2Request::new(
                    gas_limit,
                    chainspec.network_config.name.clone(),
                    state_root_hash,
                    parent_block_hash,
                    block_height,
                    &transaction,
                ) {
                    Ok(wasm_v2_request) => {
                        let result = wasm_v2_request.execute(
                            &execution_engine_v2,
                            state_root_hash,
                            &scratch_state,
                        );
                        match result {
                            Ok(wasm_v2_result) => {
                                info!(contract_hash=wasm_v2_result.smart_contract_addr().map(base16::encode_lower).unwrap_or_default(),
                                      pre_state_root_hash=%state_root_hash,
                                      post_state_root_hash=%wasm_v2_result.post_state_hash(),
                                      "install contract result");

                                state_root_hash = wasm_v2_result.state_root_hash();

                                artifact_builder.with_wasm_v2_result(wasm_v2_result);
                            }
                            Err(wasm_v2_error) => {
                                artifact_builder.with_wasm_v2_error(wasm_v2_error);
                            }
                        }
                    }
                    Err(ire) => {
                        debug!(%transaction_hash, ?lane_id, ?ire, "unable to get wasm v2 request");
                        artifact_builder.with_invalid_wasm_v2_request(ire);
                    }
                },
                _ => {
                    // it is currently not possible to specify a vm other than v1 or v2 on the
                    // transaction itself, so this should be unreachable
                    unreachable!("Unknown VM target")
                }
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
            state_root_hash =
                scratch_state.commit_effects(state_root_hash, hold_result.effects().clone())?;
            artifact_builder
                .with_balance_hold_result(&hold_result)
                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
        }

        // handle refunds per the chainspec determined setting.
        let refund_amount = {
            let consumed = artifact_builder.consumed();
            let refund_mode = match refund_handling {
                RefundHandling::NoRefund => {
                    if fee_handling.is_no_fee() && is_custom_payment {
                        // in no fee mode, we need to return the motes to the refund purse,
                        //  and then point the balance_identifier to the refund purse
                        // this will result in the downstream no fee handling logic
                        //  placing a hold on the correct purse.
                        balance_identifier = BalanceIdentifier::Refund;
                        Some(HandleRefundMode::RefundNoFeeCustomPayment {
                            initiator_addr: Box::new(initiator_addr.clone()),
                            limit: gas_limit.value(),
                            gas_price: current_gas_price,
                            cost,
                        })
                    } else {
                        None
                    }
                }
                RefundHandling::Burn { refund_ratio } => Some(HandleRefundMode::Burn {
                    limit: gas_limit.value(),
                    gas_price: current_gas_price,
                    cost,
                    consumed,
                    source: Box::new(balance_identifier.clone()),
                    ratio: refund_ratio,
                }),
                RefundHandling::Refund { refund_ratio } => {
                    let source = Box::new(balance_identifier.clone());
                    if is_custom_payment {
                        // in custom payment we have to do all payment handling up front.
                        // therefore, if refunds are turned on we have to transfer the refunded
                        // amount back to the specified refund purse.

                        // the refund purse for a given transaction is set to the initiator's main
                        // purse by default, but the custom payment provided by the initiator can
                        // set a different purse when executed. thus, the handle payment system
                        // contract tracks a refund purse and is handled internally at processing
                        // time. Outer logic should never assume or refer to a specific purse for
                        // purposes of refund. instead, `BalanceIdentifier::Refund` is used by outer
                        // logic, which is interpreted by inner logic to use the currently set
                        // refund purse.
                        let target = Box::new(BalanceIdentifier::Refund);
                        Some(HandleRefundMode::Refund {
                            initiator_addr: Box::new(initiator_addr.clone()),
                            limit: gas_limit.value(),
                            gas_price: current_gas_price,
                            consumed,
                            cost,
                            ratio: refund_ratio,
                            source,
                            target,
                        })
                    } else {
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
                            limit: gas_limit.value(),
                            gas_price: current_gas_price,
                            consumed,
                            cost,
                            ratio: refund_ratio,
                            source,
                        })
                    }
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
                        .commit_effects(state_root_hash, handle_refund_result.effects().clone())?;
                    artifact_builder
                        .with_handle_refund_result(&handle_refund_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;

                    refunded_amount
                }
                None => U512::zero(),
            }
        };

        // handle fees per the chainspec determined setting.
        let handle_fee_result = match fee_handling {
            FeeHandling::NoFee => {
                // in this mode, a gas hold is placed on the payer's purse.
                let amount = cost.saturating_sub(refund_amount);
                let hold_request = BalanceHoldRequest::new_gas_hold(
                    state_root_hash,
                    protocol_version,
                    balance_identifier,
                    amount,
                    insufficient_balance_handling,
                );
                let hold_result = scratch_state.balance_hold(hold_request);
                state_root_hash =
                    scratch_state.commit_effects(state_root_hash, hold_result.effects().clone())?;
                artifact_builder
                    .with_balance_hold_result(&hold_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::credit(proposer.clone(), amount, era_id),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
            FeeHandling::Burn => {
                // in this mode, the fee portion is burned.
                let amount = cost.saturating_sub(refund_amount);
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::burn(balance_identifier, Some(amount)),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
            FeeHandling::PayToProposer => {
                // in this mode, the consumed gas is paid as a fee to the block proposer
                let amount = cost.saturating_sub(refund_amount);
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::pay(
                        Box::new(initiator_addr),
                        balance_identifier,
                        BalanceIdentifier::Public(*(proposer.clone())),
                        amount,
                    ),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
            FeeHandling::Accumulate => {
                // in this mode, consumed gas is accumulated into a single purse
                // for later distribution
                let amount = cost.saturating_sub(refund_amount);
                let handle_fee_request = HandleFeeRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    protocol_version,
                    transaction_hash,
                    HandleFeeMode::pay(
                        Box::new(initiator_addr),
                        balance_identifier,
                        BalanceIdentifier::Accumulate,
                        amount,
                    ),
                );
                scratch_state.handle_fee(handle_fee_request)
            }
        };

        state_root_hash =
            scratch_state.commit_effects(state_root_hash, handle_fee_result.effects().clone())?;

        artifact_builder
            .with_handle_fee_result(&handle_fee_result)
            .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;

        // clear refund purse if it was set
        if refund_purse_active {
            // if refunds are turned on we initialize the refund purse to the initiator's main
            // purse before doing any processing. NOTE: when executed, custom payment logic
            // has the option to call set_refund_purse on the handle payment contract to set
            // up a different refund purse, if desired.
            let handle_refund_request = HandleRefundRequest::new(
                native_runtime_config.clone(),
                state_root_hash,
                protocol_version,
                transaction_hash,
                HandleRefundMode::ClearRefundPurse,
            );
            let handle_refund_result = scratch_state.handle_refund(handle_refund_request);
            if let Err(root_not_found) =
                artifact_builder.with_clear_refund_purse_result(&handle_refund_result)
            {
                if root_not_found {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                }
                warn!(
                    "{}",
                    artifact_builder.error_message().unwrap_or(
                        "unknown error encountered when attempting to clear refund purse"
                            .to_string()
                    )
                );
            }
            state_root_hash = scratch_state
                .commit_effects(state_root_hash, handle_refund_result.effects().clone())?;
        }

        artifacts.push(artifact_builder.build());
    }

    // transaction processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_tnx_processing
            .observe(txn_processing_start.elapsed().as_secs_f64());
    }

    // post-processing starts now
    let post_processing_start = Instant::now();

    // calculate and store checksums for approvals and execution effects across the transactions in
    // the block we do this so that the full set of approvals and the full set of effect metadata
    // can be verified if necessary for a given block. the block synchronizer in particular
    // depends on the existence of such checksums.
    let transaction_approvals_hashes = {
        let approvals_checksum = types::compute_approvals_checksum(transaction_ids.clone())
            .map_err(BlockExecutionError::FailedToComputeApprovalsChecksum)?;
        let execution_results_checksum = compute_execution_results_checksum(
            artifacts.iter().map(|artifact| &artifact.execution_result),
        )?;
        let mut checksum_registry = ChecksumRegistry::new();
        checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);
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
        scratch_state.commit_effects(state_root_hash, effects)?;
        transaction_ids
            .into_iter()
            .map(|id| id.approvals_hash())
            .collect()
    };

    if let Some(metrics) = metrics.as_ref() {
        metrics
            .txn_approvals_hashes_calculation
            .observe(post_processing_start.elapsed().as_secs_f64());
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

        // force undelegate delegators outside delegation limits before the auction runs
        debug!("starting forced undelegation");
        let forced_undelegate_req = ForcedUndelegateRequest::new(
            native_runtime_config.clone(),
            state_root_hash,
            protocol_version,
            block_time,
        );
        match scratch_state.forced_undelegate(forced_undelegate_req) {
            ForcedUndelegateResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash))
            }
            ForcedUndelegateResult::Failure(err) => {
                return Err(BlockExecutionError::ForcedUndelegate(err))
            }
            ForcedUndelegateResult::Success {
                post_state_hash, ..
            } => {
                state_root_hash = post_state_hash;
            }
        }
        debug!("forced undelegation success");

        debug!("committing step");
        let step_effects = match commit_step(
            native_runtime_config,
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
        state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;
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
        parent_hash,
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

    let proof_of_checksum_registry = match data_access_layer.tracking_copy(state_root_hash)? {
        Some(tc) => match tc.reader().read_with_proof(&Key::ChecksumRegistry)? {
            Some(proof) => proof,
            None => return Err(BlockExecutionError::MissingChecksumRegistry),
        },
        None => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
    };

    let approvals_hashes = Box::new(ApprovalsHashes::new(
        *block.hash(),
        transaction_approvals_hashes,
        proof_of_checksum_registry,
    ));

    // processing is finished now
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_post_processing
            .observe(post_processing_start.elapsed().as_secs_f64());
        metrics
            .exec_block_total
            .observe(start.elapsed().as_secs_f64());
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
    input_transaction: Transaction,
) -> SpeculativeExecutionResult
where
    S: StateProvider,
{
    let transaction_config = &chainspec.transaction_config;
    let maybe_transaction =
        MetaTransaction::from_transaction(&input_transaction, transaction_config);
    if let Err(error) = maybe_transaction {
        return SpeculativeExecutionResult::invalid_transaction(error);
    }
    let transaction = maybe_transaction.unwrap();
    let state_root_hash = block_header.state_root_hash();
    let parent_block_hash = block_header.block_hash();
    let block_height = block_header.height();
    let block_time = block_header
        .timestamp()
        .saturating_add(chainspec.core_config.minimum_block_time);
    let gas_limit = match input_transaction.gas_limit(chainspec, transaction.transaction_lane()) {
        Ok(gas_limit) => gas_limit,
        Err(_) => {
            return SpeculativeExecutionResult::invalid_gas_limit(input_transaction);
        }
    };

    if transaction.is_deploy_transaction() {
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
                initiator_addr,
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
            let wasm_v1_result =
                match WasmV1Request::new_session(block_info, gas_limit, &session_input_data) {
                    Ok(wasm_v1_request) => {
                        execution_engine_v1.execute(state_provider, wasm_v1_request)
                    }
                    Err(error) => WasmV1Result::invalid_executable_item(gas_limit, error),
                };
            SpeculativeExecutionResult::WasmV1(Box::new(utils::spec_exec_from_wasm_v1_result(
                wasm_v1_result,
                block_header.block_hash(),
            )))
        }
    } else {
        SpeculativeExecutionResult::ReceivedV1Transaction
    }
}

fn invoked_contract_will_pay(
    state_provider: &ScratchGlobalState,
    state_root_hash: Digest,
    transaction: &MetaTransaction,
) -> Result<Option<EntityAddr>, StateResultError> {
    let (hash_addr, entry_point_name) = match transaction.contract_direct_address() {
        None => {
            return Err(StateResultError::ValueNotFound(
                "contract direct address not found".to_string(),
            ))
        }
        Some((hash_addr, entry_point_name)) => (hash_addr, entry_point_name),
    };
    let entity_addr = EntityAddr::new_smart_contract(hash_addr);
    let entry_point_request = EntryPointRequest::new(state_root_hash, entry_point_name, hash_addr);
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
