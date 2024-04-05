use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{ExecutionEngineV1, WasmV1Request, WasmV1Result};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        balance::BalanceHandling, AuctionMethod, BalanceHoldKind, BalanceHoldRequest,
        BalanceIdentifier, BalanceRequest, BiddingRequest, BlockRewardsRequest, BlockRewardsResult,
        DataAccessLayer, EraValidatorsRequest, EraValidatorsResult, EvictItem, FeeRequest,
        FeeResult, FlushRequest, HandleFeeMode, HandleFeeRequest, HandleRefundMode,
        HandleRefundRequest, InsufficientBalanceHandling, ProofHandling, PruneRequest, PruneResult,
        StepRequest, StepResult, TransferRequest,
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
    BlockHeader, BlockTime, BlockV2, CLValue, CategorizedTransaction, Chainspec, ChecksumRegistry,
    Digest, EraEndV2, EraId, FeeHandling, Gas, GasLimited, HoldsEpoch, Key, ProtocolVersion,
    PublicKey, RefundHandling, Transaction, TransactionCategory, U512,
};

use super::{
    types::{SpeculativeExecutionResult, StepOutcome},
    utils::{self, calculate_prune_eras},
    BlockAndExecutionArtifacts, BlockExecutionError, ExecutionPreState, Metrics,
    APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
};
use crate::{
    components::fetcher::FetchItem,
    contract_runtime::types::ExecutionArtifactBuilder,
    types::{self, Chunkable, ExecutableBlock, InternalEraReport},
};

const ACCOUNT_SESSION: bool = true;
const DIRECT_CONTRACT: bool = !ACCOUNT_SESSION;
const STANDARD_PAYMENT: bool = true;
const CUSTOM_PAYMENT: bool = !STANDARD_PAYMENT;

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
    execution_engine_v1: &ExecutionEngineV1,
    chainspec: &Chainspec,
    metrics: Option<Arc<Metrics>>,
    execution_pre_state: ExecutionPreState,
    executable_block: ExecutableBlock,
    key_block_height_for_activation_point: u64,
    current_gas_price: u8,
    next_era_gas_price: Option<u8>,
) -> Result<BlockAndExecutionArtifacts, BlockExecutionError> {
    if executable_block.height != execution_pre_state.next_block_height() {
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

    // scrape variables from execution pre state
    let parent_hash = execution_pre_state.parent_hash();
    let parent_seed = execution_pre_state.parent_seed();
    let pre_state_root_hash = execution_pre_state.pre_state_root_hash();
    let mut state_root_hash = pre_state_root_hash; // initial state root is parent's state root

    // scrape variables from executable block
    let block_time = BlockTime::new(executable_block.timestamp.millis());
    let proposer = executable_block.proposer.clone();
    let mut artifacts = Vec::with_capacity(executable_block.transactions.len());

    // set up accounting variables / settings
    let holds_epoch =
        HoldsEpoch::from_block_time(block_time, chainspec.core_config.balance_hold_interval);
    let balance_handling = BalanceHandling::Available { holds_epoch };
    let insufficient_balance_handling = InsufficientBalanceHandling::HoldRemaining;
    let refund_handling = chainspec.core_config.refund_handling;
    let fee_handling = chainspec.core_config.fee_handling;

    // get scratch state, which must be used for all processing and post processing data
    // requirements.
    let scratch_state = data_access_layer.get_scratch_global_state();

    // pre processing is finished
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

    for transaction in executable_block.transactions {
        let mut artifact_builder = ExecutionArtifactBuilder::new(&transaction);

        let initiator_addr = transaction.initiator_addr();
        let transaction_hash = transaction.hash();
        let runtime_args = transaction.session_args().clone();
        let entry_point = transaction.entry_point();
        let authorization_keys = transaction.authorization_keys();

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
        consumed is determined after execution and is used for refund & fee post processing.
        */

        // NOTE: this is the allowed computation limit   (gas limit)
        let gas_limit = match transaction.gas_limit(chainspec) {
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
        let cost = match transaction.gas_cost(chainspec, current_gas_price) {
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
        let refund_purse_active = !is_standard_payment && !refund_handling.skip_refund();
        // set up the refund purse for this transaction, if custom payment && refunds are on
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
                continue; // no reason to commit the effects, move on
            }
            state_root_hash =
                scratch_state.commit(state_root_hash, handle_refund_result.effects().clone())?;
        }

        let balance_identifier = {
            let is_account_session = transaction.is_account_session();
            //  if standard payment & is session based...use account main purse
            //  if standard payment & is targeting a contract
            //      load contract & check entry point, if it pays use contract main purse
            //  if custom payment, attempt execute custom payment
            //      if custom payment fails, take remaining balance up to amount
            //      currently contracts cannot provide custom payment, but possible future use
            match (is_account_session, is_standard_payment) {
                (ACCOUNT_SESSION, STANDARD_PAYMENT) => {
                    // this is the typical scenario; the initiating account pays using its main
                    // purse
                    trace!(%transaction_hash, "account session with standard payment");
                    initiator_addr.clone().into()
                }
                (ACCOUNT_SESSION, CUSTOM_PAYMENT) => {
                    // the initiating account will pay, but wants to do so with a different purse or
                    // in a custom way. If anything goes wrong, penalize the sender, do not execute
                    let custom_payment_gas_limit = Gas::new(
                        // TODO: this should have it's own chainspec value; in the meantime
                        // using a multiple of a small value.
                        chainspec.transaction_config.native_transfer_minimum_motes * 5,
                    );
                    let pay_result = match WasmV1Request::new_custom_payment(
                        state_root_hash,
                        block_time,
                        custom_payment_gas_limit,
                        &transaction,
                    ) {
                        Ok(pay_request) => execution_engine_v1.execute(&scratch_state, pay_request),
                        Err(error) => {
                            WasmV1Result::invalid_executable_item(custom_payment_gas_limit, error)
                        }
                    };
                    let balance_identifier = {
                        if pay_result.error().is_some() {
                            BalanceIdentifier::PenalizedAccount(initiator_addr.account_hash())
                        } else {
                            BalanceIdentifier::Payment
                        }
                    };
                    state_root_hash =
                        scratch_state.commit(state_root_hash, pay_result.effects().clone())?;
                    artifact_builder
                        .with_wasm_v1_result(pay_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    trace!(%transaction_hash, ?balance_identifier, "account session with custom payment");
                    balance_identifier
                }
                (DIRECT_CONTRACT, STANDARD_PAYMENT) => {
                    // TODO: get the contract, check the entry point indicated in the transaction
                    //      if the contract pays for itself, use its main purse
                    // <-- contracts paying for things wire up goes here -->
                    // use scratch_state to read the contract & check it here
                    // if the contract does not exist, the entrypoint does not exist,
                    //      the entrypoint does not pay for itself, and every other sad path
                    // outcome...      penalize the sender, do not execute
                    debug!("direct contract invocation is currently unsupported; penalize the account that sent it.");
                    BalanceIdentifier::PenalizedAccount(initiator_addr.account_hash())
                }
                (DIRECT_CONTRACT, CUSTOM_PAYMENT) => {
                    // currently not supported. penalize the sender, do not execute.
                    warn!("direct contract invocation is currently unsupported; penalize the account that sent it.");
                    BalanceIdentifier::PenalizedAccount(initiator_addr.account_hash())
                }
            }
        };

        let initial_balance_result = scratch_state.balance(BalanceRequest::new(
            state_root_hash,
            protocol_version,
            balance_identifier.clone(),
            balance_handling,
            ProofHandling::NoProofs,
        ));

        let allow_execution = {
            let is_not_penalized = !balance_identifier.is_penalty();
            let sufficient_balance = initial_balance_result.is_sufficient(cost);
            trace!(%transaction_hash, ?sufficient_balance, ?is_not_penalized, "payment preprocessing");
            is_not_penalized && sufficient_balance
        };

        if allow_execution {
            if is_standard_payment {
                // place a processing hold on the paying account to prevent double spend.
                let hold_amount = cost;
                let hold_request = BalanceHoldRequest::new_processing_hold(
                    state_root_hash,
                    protocol_version,
                    balance_identifier.clone(),
                    hold_amount,
                    block_time,
                    holds_epoch,
                    insufficient_balance_handling,
                );
                let hold_result = scratch_state.balance_hold(hold_request);
                state_root_hash =
                    scratch_state.commit(state_root_hash, hold_result.effects().clone())?;
                artifact_builder
                    .with_balance_hold_result(&hold_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
            }

            let category = transaction.category();
            trace!(%transaction_hash, ?category, "eligible for execution");
            match category {
                TransactionCategory::Mint => {
                    let transfer_result =
                        scratch_state.transfer(TransferRequest::with_runtime_args(
                            native_runtime_config.clone(),
                            state_root_hash,
                            holds_epoch,
                            protocol_version,
                            transaction_hash,
                            initiator_addr.clone(),
                            authorization_keys,
                            runtime_args,
                        ));
                    let consumed = gas_limit;
                    state_root_hash =
                        scratch_state.commit(state_root_hash, transfer_result.effects().clone())?;
                    artifact_builder
                        .with_added_consumed(consumed)
                        .with_transfer_result(transfer_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                }
                TransactionCategory::Auction => {
                    match AuctionMethod::from_parts(
                        entry_point,
                        &runtime_args,
                        holds_epoch,
                        chainspec,
                    ) {
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
                            state_root_hash = scratch_state
                                .commit(state_root_hash, bidding_result.effects().clone())?;
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
                TransactionCategory::Standard | TransactionCategory::InstallUpgrade => {
                    let wasm_v1_start = Instant::now();
                    match WasmV1Request::new_session(
                        state_root_hash,
                        block_time,
                        gas_limit,
                        &transaction,
                    ) {
                        Ok(wasm_v1_request) => {
                            trace!(%transaction_hash, ?category, ?wasm_v1_request, "able to get wasm v1 request");
                            let wasm_v1_result =
                                execution_engine_v1.execute(&scratch_state, wasm_v1_request);
                            trace!(%transaction_hash, ?category, ?wasm_v1_result, "able to get wasm v1 result");
                            state_root_hash = scratch_state
                                .commit(state_root_hash, wasm_v1_result.effects().clone())?;
                            // note: consumed is scraped from wasm_v1_result along w/ other fields
                            artifact_builder
                                .with_wasm_v1_result(wasm_v1_result)
                                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                        }
                        Err(ire) => {
                            debug!(%transaction_hash, ?category, ?ire, "unable to get wasm v1  request");
                            artifact_builder.with_invalid_wasm_v1_request(&ire);
                        }
                    };
                    if let Some(metrics) = metrics.as_ref() {
                        metrics
                            .exec_wasm_v1
                            .observe(wasm_v1_start.elapsed().as_secs_f64());
                    }
                }
            }
        } else {
            debug!(%transaction_hash, "not eligible for execution");
        }

        // clear all holds on the balance_identifier purse before payment processing
        {
            let hold_request = BalanceHoldRequest::new_clear(
                state_root_hash,
                protocol_version,
                block_time,
                BalanceHoldKind::All,
                balance_identifier.clone(),
                holds_epoch,
            );
            let hold_result = scratch_state.balance_hold(hold_request);
            state_root_hash =
                scratch_state.commit(state_root_hash, hold_result.effects().clone())?;
            artifact_builder
                .with_balance_hold_result(&hold_result)
                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
        }

        // handle refunds per the chainspec determined setting.
        let refund_amount = {
            let consumed = artifact_builder.consumed();
            let refund_mode = match refund_handling {
                RefundHandling::NoRefund => None,
                RefundHandling::Burn { refund_ratio } => Some(HandleRefundMode::Burn {
                    limit: gas_limit.value(),
                    gas_price: current_gas_price,
                    cost,
                    consumed,
                    source: Box::new(balance_identifier.clone()),
                    ratio: refund_ratio,
                }),
                RefundHandling::Refund { refund_ratio } => {
                    if transaction.is_standard_payment() {
                        Some(HandleRefundMode::RefundAmount {
                            limit: gas_limit.value(),
                            gas_price: current_gas_price,
                            consumed,
                            cost,
                            ratio: refund_ratio,
                            source: Box::new(balance_identifier.clone()),
                        })
                    } else {
                        Some(HandleRefundMode::Refund {
                            initiator_addr: Box::new(initiator_addr.clone()),
                            limit: gas_limit.value(),
                            gas_price: current_gas_price,
                            consumed,
                            cost,
                            ratio: refund_ratio,
                            // as this is currently behind a custom payment check,
                            // the source is always BalanceIdentifier::Payment
                            source: Box::new(balance_identifier.clone()),
                            target: Box::new(BalanceIdentifier::Refund),
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
                        .commit(state_root_hash, handle_refund_result.effects().clone())?;
                    artifact_builder
                        .with_handle_refund_result(&handle_refund_result)
                        .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    refunded_amount
                }
                None => U512::zero(),
            }
        };

        // handle fees per the chainspec determined setting.
        match fee_handling {
            FeeHandling::NoFee => {
                // in this mode, a gas hold for cost - refund (if any) is placed
                // on the payer's purse.
                let amount = cost.saturating_sub(refund_amount);
                let hold_request = BalanceHoldRequest::new_gas_hold(
                    state_root_hash,
                    protocol_version,
                    balance_identifier,
                    amount,
                    block_time,
                    holds_epoch,
                    insufficient_balance_handling,
                );
                let hold_result = scratch_state.balance_hold(hold_request);
                state_root_hash =
                    scratch_state.commit(state_root_hash, hold_result.effects().clone())?;
                artifact_builder
                    .with_balance_hold_result(&hold_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
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
                let handle_fee_result = scratch_state.handle_fee(handle_fee_request);
                state_root_hash =
                    scratch_state.commit(state_root_hash, handle_fee_result.effects().clone())?;
                artifact_builder
                    .with_handle_fee_result(&handle_fee_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
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
                        holds_epoch,
                    ),
                );
                let handle_fee_result = scratch_state.handle_fee(handle_fee_request);
                state_root_hash =
                    scratch_state.commit(state_root_hash, handle_fee_result.effects().clone())?;
                artifact_builder
                    .with_handle_fee_result(&handle_fee_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
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
                        holds_epoch,
                    ),
                );
                let handle_fee_result = scratch_state.handle_fee(handle_fee_request);
                state_root_hash =
                    scratch_state.commit(state_root_hash, handle_fee_result.effects().clone())?;
                artifact_builder
                    .with_handle_fee_result(&handle_fee_result)
                    .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
            }
        }

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
            state_root_hash =
                scratch_state.commit(state_root_hash, handle_refund_result.effects().clone())?;
        }

        artifacts.push(artifact_builder.build());
    }

    // transaction processing is finished
    if let Some(metrics) = metrics.as_ref() {
        metrics
            .exec_block_tnx_processing
            .observe(txn_processing_start.elapsed().as_secs_f64());
    }

    // post processing starts now
    let post_processing_start = Instant::now();

    // calculate and store checksums for approvals and execution effects across the transactions in
    // the block we do this so that the full set of approvals and the full set of effect meta
    // data can be verified if necessary for a given block. the block synchronizer in particular
    // depends on the existence of such checksums.
    let txns_approvals_hashes = {
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
        scratch_state.commit(state_root_hash, effects)?;
        transaction_ids
            .into_iter()
            .map(|id| id.approvals_hash())
            .collect()
    };

    // Pay out  ̶b̶l̶o̶c̶k̶ e͇r͇a͇ rewards
    // NOTE: despite the name, these rewards are currently paid out per ERA not per BLOCK
    // at one point, they were going to be paid out per block (and might be in the future)
    // but it ended up settling on per era. the behavior is driven by Some / None
    // thus if in future the calling logic passes rewards per block it should just work as is.
    // This auto-commits.
    if let Some(rewards) = &executable_block.rewards {
        // Pay out block fees, if relevant. This auto-commits
        {
            let fee_req = FeeRequest::new(
                native_runtime_config.clone(),
                state_root_hash,
                protocol_version,
                block_time,
                holds_epoch,
            );
            match scratch_state.distribute_fees(fee_req) {
                FeeResult::RootNotFound => {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                }
                FeeResult::Failure(fer) => return Err(BlockExecutionError::DistributeFees(fer)),
                FeeResult::Success {
                    post_state_hash, ..
                } => {
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
                state_root_hash = post_state_hash;
            }
        }
    }

    // if era report is some, this is a switch block. a series of end-of-era extra processing must
    // transpire before this block is entirely finished.
    let step_outcome = if let Some(era_report) = &executable_block.era_report {
        // step processing starts now
        let step_processing_start = Instant::now();
        let step_effects = match commit_step(
            native_runtime_config,
            &scratch_state,
            metrics.clone(),
            protocol_version,
            state_root_hash,
            era_report.clone(),
            executable_block.timestamp.millis(),
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

        let era_validators_req = EraValidatorsRequest::new(state_root_hash, protocol_version);
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
    // at the end to avoid an read ordering issue during block execution.
    if let Some(previous_block_height) = executable_block.height.checked_sub(1) {
        if let Some(keys_to_prune) = calculate_prune_eras(
            activation_point_era_id,
            key_block_height_for_activation_point,
            previous_block_height,
            prune_batch_size,
        ) {
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
        }
    }

    {
        // Finally, the new state-root-hash from the cumulative changes to global state is
        // returned when they are written to LMDB.
        state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;
        // Flush once, after all data mutation.
        let flush_req = FlushRequest::new();
        let flush_result = data_access_layer.flush(flush_req);
        if let Err(gse) = flush_result.as_error() {
            error!("failed to flush lmdb");
            return Err(BlockExecutionError::Lmdb(gse));
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
        executable_block.height,
        protocol_version,
        (*proposer).clone(),
        executable_block.mint,
        executable_block.auction,
        executable_block.install_upgrade,
        executable_block.standard,
        executable_block.rewarded_signatures,
        current_gas_price,
    ));

    // TODO: this should just use the data_access_layer.query mechanism to avoid
    // leaking data provisioning abstraction
    let proof_of_checksum_registry = match data_access_layer.tracking_copy(state_root_hash)? {
        Some(tc) => match tc.reader().read_with_proof(&Key::ChecksumRegistry)? {
            Some(proof) => proof,
            None => return Err(BlockExecutionError::MissingChecksumRegistry),
        },
        None => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
    };

    let approvals_hashes = Box::new(ApprovalsHashes::new(
        *block.hash(),
        txns_approvals_hashes,
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
    transaction: Transaction,
) -> SpeculativeExecutionResult
where
    S: StateProvider,
{
    let state_root_hash = block_header.state_root_hash();
    let block_time = block_header
        .timestamp()
        .saturating_add(chainspec.core_config.minimum_block_time);
    let gas_limit = match transaction.gas_limit(chainspec) {
        Ok(gas_limit) => gas_limit,
        Err(_) => {
            return SpeculativeExecutionResult::invalid_gas_limit(transaction);
        }
    };
    let wasm_v1_result = match WasmV1Request::new_session(
        *state_root_hash,
        block_time.into(),
        gas_limit,
        &transaction,
    ) {
        Ok(wasm_v1_request) => execution_engine_v1.execute(state_provider, wasm_v1_request),
        Err(error) => WasmV1Result::invalid_executable_item(gas_limit, error),
    };
    SpeculativeExecutionResult::WasmV1(utils::spec_exec_from_wasm_v1_result(wasm_v1_result))
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
