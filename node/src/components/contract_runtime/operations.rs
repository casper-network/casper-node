use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{
    self,
    execution_result::{ExecutionResultAndMessages, ExecutionResults},
    DeployItem, ExecuteRequest, ExecutionEngineV1, ExecutionResult as EngineExecutionResult,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        balance::BalanceHandling, AuctionMethod, BalanceHoldRequest, BalanceIdentifier,
        BalanceRequest, BiddingRequest, BlockRewardsRequest, BlockRewardsResult, DataAccessLayer,
        EraValidatorsRequest, EraValidatorsResult, EvictItem, FeeRequest, FeeResult, FlushRequest,
        InsufficientBalanceHandling, PruneRequest, PruneResult, StepRequest, StepResult,
        TransferRequest,
    },
    global_state::{
        error::Error as GlobalStateError,
        state::{
            lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, ScratchProvider,
            StateProvider, StateReader,
        },
    },
    system::runtime_native::Config as NativeRuntimeConfig,
};
use casper_types::{
    binary_port::SpeculativeExecutionResult,
    bytesrepr::{self, ToBytes, U32_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::{Effects, ExecutionResult, TransformKindV2, TransformV2},
    system::mint::BalanceHoldAddrTag,
    BlockTime, BlockV2, CLValue, CategorizedTransaction, Chainspec, ChecksumRegistry, DeployHash,
    Digest, EraEndV2, EraId, FeeHandling, GasLimited, Key, ProtocolVersion, PublicKey, Transaction,
    TransactionCategory, U512,
};

use crate::{
    components::{
        contract_runtime::{
            error::BlockExecutionError, types::StepOutcome, BlockAndExecutionArtifacts,
            ExecutionPreState, Metrics, SpeculativeExecutionState, APPROVALS_CHECKSUM_NAME,
            EXECUTION_RESULTS_CHECKSUM_NAME,
        },
        fetcher::FetchItem,
    },
    contract_runtime::{types::ExecutionArtifactOutcome, utils::calculate_prune_eras},
    types::{self, Chunkable, ExecutableBlock, InternalEraReport},
};

use super::{ExecutionArtifact, ExecutionArtifacts};

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
) -> Result<BlockAndExecutionArtifacts, BlockExecutionError> {
    if executable_block.height != execution_pre_state.next_block_height() {
        return Err(BlockExecutionError::WrongBlockHeight {
            executable_block: Box::new(executable_block),
            execution_pre_state: Box::new(execution_pre_state),
        });
    }

    let protocol_version = chainspec.protocol_version();
    let activation_point_era_id = chainspec.protocol_config.activation_point.era_id();
    let prune_batch_size = chainspec.core_config.prune_batch_size;
    let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);

    let pre_state_root_hash = execution_pre_state.pre_state_root_hash();
    let parent_hash = execution_pre_state.parent_hash();
    let parent_seed = execution_pre_state.parent_seed();

    let mut state_root_hash = pre_state_root_hash;
    let mut artifacts = ExecutionArtifacts::with_capacity(executable_block.transactions.len());
    let block_time = executable_block.timestamp.millis();
    let balance_handling = BalanceHandling::Available {
        block_time,
        hold_interval: chainspec.core_config.balance_hold_interval.millis(),
    };
    let holds_epoch = Some(chainspec.balance_holds_epoch(executable_block.timestamp));

    let start = Instant::now();

    let scratch_state = data_access_layer.get_scratch_global_state();
    let mut effects = Effects::new();

    let system_costs = chainspec.system_costs_config;
    let insufficient_balance_handling = InsufficientBalanceHandling::HoldRemaining;
    let gas_price = Some(1); // < --TODO: this is where Karan's calculated gas price needs to be used

    for transaction in &executable_block.transactions {
        let transaction_hash = transaction.hash();
        let transaction_header = transaction.header();
        let runtime_args = transaction.session_args().clone();
        let entry_point = transaction.entry_point();
        let balance_identifier: BalanceIdentifier = transaction.initiator_addr().into();

        // NOTE: this is the actual adjusted cost   (gas limit * gas price)
        // NOT the allowed computation limit        (gas limit)
        let cost = match transaction.gas_limit(&system_costs, gas_price) {
            Ok(gas) => gas.value(),
            Err(ite) => {
                artifacts.push_invalid_transaction(
                    transaction_hash,
                    transaction_header.clone(),
                    ite.clone(),
                );
                debug!(%ite, "invalid transaction");
                continue;
            }
        };

        let initial_balance_result = scratch_state.balance(BalanceRequest::new(
            state_root_hash,
            protocol_version,
            balance_identifier.clone(),
            balance_handling,
        ));

        let is_sufficient_balance = initial_balance_result.is_sufficient(cost);
        let authorization_keys = transaction.authorization_keys();

        match (transaction.category(), is_sufficient_balance) {
            (_, false) => {
                debug!(
                    "skipping execution of {} due to insufficient balance",
                    transaction_hash
                );
            }
            (TransactionCategory::Mint, _) => {
                let transfer_result = scratch_state.transfer(TransferRequest::with_runtime_args(
                    native_runtime_config.clone(),
                    state_root_hash,
                    holds_epoch,
                    protocol_version,
                    transaction_hash,
                    transaction.initiator_addr().account_hash(),
                    authorization_keys.clone(),
                    runtime_args.clone(),
                    cost,
                ));
                match artifacts.push_transfer_result(
                    transaction_hash,
                    transaction_header.clone(),
                    transfer_result,
                    cost,
                ) {
                    ExecutionArtifactOutcome::RootNotFound => {
                        return Err(BlockExecutionError::RootNotFound(state_root_hash))
                    }
                    ExecutionArtifactOutcome::Failure => {
                        continue;
                    }
                    ExecutionArtifactOutcome::Success(transfer_effects) => {
                        effects.append(transfer_effects.clone());
                    }
                }
            }
            (TransactionCategory::Auction, _) => {
                let auction_method = match AuctionMethod::from_parts(
                    entry_point,
                    &runtime_args,
                    holds_epoch,
                    chainspec,
                ) {
                    Ok(auction_method) => auction_method,
                    Err(_) => {
                        artifacts.push_auction_method_failure(
                            transaction_hash,
                            transaction_header.clone(),
                            cost,
                        );
                        continue;
                    }
                };
                let bidding_result = scratch_state.bidding(BiddingRequest::new(
                    native_runtime_config.clone(),
                    state_root_hash,
                    block_time,
                    protocol_version,
                    transaction_hash,
                    transaction.initiator_addr().account_hash(),
                    authorization_keys.clone(),
                    auction_method,
                ));
                match artifacts.push_bidding_result(
                    transaction_hash,
                    transaction_header.clone(),
                    bidding_result,
                    cost,
                ) {
                    ExecutionArtifactOutcome::RootNotFound => {
                        return Err(BlockExecutionError::RootNotFound(state_root_hash))
                    }
                    ExecutionArtifactOutcome::Failure => {
                        continue;
                    }
                    ExecutionArtifactOutcome::Success(bidding_effects) => {
                        effects.append(bidding_effects.clone());
                    }
                }
            }
            (TransactionCategory::Standard, _) | (TransactionCategory::InstallUpgrade, _) => {
                // TODO: i think fraser is doing the Transaction::V1(_) fixing here, but either way
                // it needs to get done soonish to complete this work. All of this section needs
                // a bit of a re-work to allow the rest of this logic to move further away from
                // the legacy notions.
                let (deploy_hash, deploy) = match transaction {
                    Transaction::Deploy(deploy) => {
                        let deploy_hash = *deploy.hash();
                        (deploy_hash, deploy)
                    }
                    Transaction::V1(_) => continue,
                };

                let deploy_header = deploy.header().clone();
                let execute_request = ExecuteRequest::new(
                    state_root_hash,
                    block_time,
                    vec![DeployItem::from(deploy.clone())],
                    protocol_version,
                    PublicKey::clone(&executable_block.proposer),
                );

                let exec_result = execute(
                    &scratch_state,
                    execution_engine_v1,
                    metrics.clone(),
                    execute_request,
                )?;

                trace!(?deploy_hash, ?exec_result, "transaction execution result");
                // As for now a given state is expected to exist.
                let (state_hash, execution_result, messages) = commit_execution_results(
                    &scratch_state,
                    metrics.clone(),
                    state_root_hash,
                    deploy_hash,
                    exec_result,
                )?;
                artifacts.push(ExecutionArtifact::deploy(
                    deploy_hash,
                    deploy_header,
                    execution_result,
                    messages,
                ));
                state_root_hash = state_hash;
            }
        }

        // handle payment per the chainspec determined fee setting
        match chainspec.core_config.fee_handling {
            FeeHandling::NoFee => {
                // this is the "fee elimination" model...a BalanceHold for the full cost is placed
                // on the initiator's purse.
                let hold_result = scratch_state.balance_hold(BalanceHoldRequest::new(
                    state_root_hash,
                    protocol_version,
                    balance_identifier.clone(),
                    BalanceHoldAddrTag::Gas,
                    cost,
                    BlockTime::new(block_time),
                    chainspec.core_config.balance_hold_interval,
                    insufficient_balance_handling,
                ));
                match artifacts.push_hold_result(
                    transaction_hash,
                    transaction_header.clone(),
                    hold_result,
                ) {
                    ExecutionArtifactOutcome::RootNotFound => {
                        return Err(BlockExecutionError::RootNotFound(state_root_hash))
                    }
                    ExecutionArtifactOutcome::Failure => {
                        continue;
                    }
                    ExecutionArtifactOutcome::Success(hold_effects) => {
                        effects.append(hold_effects.clone())
                    }
                }
            }
            FeeHandling::PayToProposer => {
                // this is the current mainnet mechanism...pay up front
                // finalize at the end
                // we have the proposer of this block...just deposit to them
            }
            FeeHandling::Accumulate => {
                // this is a variation on PayToProposer that was added for
                // for some private networks...the fees are all accumulated
                // and distributed to administrative accounts as part of fee
                // distribution. So, we just send the payment to the accumulator
                // purse and move on.
            }
            FeeHandling::Burn => {
                // this is a new variation that is not currently supported.
                // this is for future use...but it is very simple...the
                // fees are simply burned, lowering total supply.
            }
        }
    }

    // calculate and store checksums for approvals and execution effects across the transactions in
    // the block we do this so that the full set of approvals and the full set of effect meta
    // data can be verified if necessary for a given block. the block synchronizer in particular
    // depends on the existence of such checksums.
    let transactions_approvals_hashes = {
        let mut checksum_registry = ChecksumRegistry::new();

        let txn_ids = executable_block
            .transactions
            .iter()
            .map(Transaction::fetch_id)
            .collect_vec();

        let approvals_checksum = types::compute_approvals_checksum(txn_ids.clone())
            .map_err(BlockExecutionError::FailedToComputeApprovalsChecksum)?;

        checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);

        // Write the deploy approvals' and execution results' checksums to global state.
        let execution_results_checksum =
            compute_execution_results_checksum(artifacts.execution_results().into_iter())?;
        checksum_registry.insert(EXECUTION_RESULTS_CHECKSUM_NAME, execution_results_checksum);

        effects.push(TransformV2::new(
            Key::ChecksumRegistry,
            TransformKindV2::Write(
                CLValue::from_t(checksum_registry)
                    .map_err(BlockExecutionError::ChecksumRegistryToCLValue)?
                    .into(),
            ),
        ));

        txn_ids.into_iter().map(|id| id.approvals_hash()).collect()
    };

    // After all transaction processing has been completed, commit all of the effects.
    scratch_state.commit(state_root_hash, effects)?;

    // Pay out block fees, if relevant.
    {
        let fee_req = FeeRequest::new(
            native_runtime_config.clone(),
            state_root_hash,
            protocol_version,
            holds_epoch,
        );
        match scratch_state.distribute_fees(fee_req) {
            FeeResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash))
            }
            FeeResult::Failure(fer) => return Err(BlockExecutionError::DistributeFees(fer)),
            FeeResult::Success {
                //transfers: fee_transfers,
                post_state_hash,
                ..
            } => {
                //transfers.extend(fee_transfers);
                state_root_hash = post_state_hash;
                // TODO: looks like effects & transfer records are associated with the
                // ExecutionResult struct which assumes they were caused by a
                // deploy. however, systemic operations produce effects and transfer
                // records also.
            }
        }
    }

    // Update exec_block metric BEFORE determining per era things such as era rewards and step.
    // the commit_step function handles the metrics for step
    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // Pay out  ̶b̶l̶o̶c̶k̶ e͇r͇a͇ rewards
    // NOTE: despite the name, these rewards are currently paid out per ERA not per BLOCK
    // at one point, they were going to be paid out per block (and might be in the future)
    // but it ended up settling on per era. the behavior is driven by Some / None
    // thus if in future the calling logic passes rewards per block it should just work as is.
    if let Some(rewards) = &executable_block.rewards {
        let rewards_req = BlockRewardsRequest::new(
            native_runtime_config.clone(),
            state_root_hash,
            protocol_version,
            block_time,
            rewards.clone(),
        );
        match scratch_state.distribute_block_rewards(rewards_req) {
            BlockRewardsResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash))
            }
            BlockRewardsResult::Failure(bre) => {
                return Err(BlockExecutionError::DistributeBlockRewards(bre))
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
        let step_effects = match commit_step(
            native_runtime_config,
            &scratch_state,
            metrics,
            protocol_version,
            state_root_hash,
            era_report.clone(),
            executable_block.timestamp.millis(),
            executable_block.era_id.successor(),
        ) {
            StepResult::RootNotFound => {
                return Err(BlockExecutionError::RootNotFound(state_root_hash))
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

        state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;

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

        Some(StepOutcome {
            step_effects,
            upcoming_era_validators,
        })
    } else {
        // Finally, the new state-root-hash from the cumulative changes to global state is
        // returned when they are written to LMDB.
        state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;
        None
    };

    // Pruning
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
            match data_access_layer.prune(request) {
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

    // Flush once, after all data mutation.
    let flush_req = FlushRequest::new();
    let flush_result = data_access_layer.flush(flush_req);
    if let Err(gse) = flush_result.as_error() {
        error!("failed to flush lmdb");
        return Err(BlockExecutionError::Lmdb(gse));
    }

    let maybe_next_era_validator_weights: Option<BTreeMap<PublicKey, U512>> = {
        let next_era_id = executable_block.era_id.successor();
        step_outcome.as_ref().and_then(
            |StepOutcome {
                 upcoming_era_validators,
                 ..
             }| upcoming_era_validators.get(&next_era_id).cloned(),
        )
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
            Some(next_era_validator_weights),
        ) => Some(EraEndV2::new(
            equivocators,
            inactive_validators,
            next_era_validator_weights,
            executable_block.rewards.unwrap_or_default(),
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
        (*executable_block.proposer).clone(),
        executable_block.mint,
        executable_block.auction,
        executable_block.install_upgrade,
        executable_block.standard,
        executable_block.rewarded_signatures,
    ));

    let tc = match data_access_layer.tracking_copy(state_root_hash) {
        Ok(Some(tc)) => tc,
        Ok(None) => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
        Err(gse) => return Err(BlockExecutionError::Lmdb(gse)),
    };

    let approvals_hashes = {
        let proof_of_checksum_registry = match tc.reader().read_with_proof(&Key::ChecksumRegistry) {
            Ok(Some(proof)) => proof,
            Ok(None) => return Err(BlockExecutionError::MissingChecksumRegistry),
            Err(gse) => return Err(BlockExecutionError::Lmdb(gse)),
        };

        Box::new(ApprovalsHashes::new(
            *block.hash(),
            transactions_approvals_hashes,
            proof_of_checksum_registry,
        ))
    };

    let execution_artifacts = artifacts.take();

    Ok(BlockAndExecutionArtifacts {
        block,
        approvals_hashes,
        execution_artifacts,
        step_outcome,
    })
}

/// Commits the execution results.
fn commit_execution_results(
    scratch_state: &ScratchGlobalState,
    metrics: Option<Arc<Metrics>>,
    state_root_hash: Digest,
    deploy_hash: DeployHash,
    execution_results: ExecutionResults,
) -> Result<(Digest, ExecutionResult, Messages), BlockExecutionError> {
    let ee_execution_result = execution_results
        .into_iter()
        .exactly_one()
        .map_err(|_| BlockExecutionError::MoreThanOneExecutionResult)?;

    let effects = match &ee_execution_result {
        EngineExecutionResult::Success { effects, cost, .. } => {
            // We do want to see the deploy hash and cost in the logs.
            // We don't need to see the effects in the logs.
            debug!(?deploy_hash, %cost, "execution succeeded");
            effects.clone()
        }
        EngineExecutionResult::Failure {
            error,
            effects,
            cost,
            ..
        } => {
            // Failure to execute a contract is a user error, not a system error.
            // We do want to see the deploy hash, error, and cost in the logs.
            // We don't need to see the effects in the logs.
            debug!(?deploy_hash, ?error, %cost, "execution failure");
            effects.clone()
        }
    };
    let new_state_root = commit_transforms(scratch_state, metrics, state_root_hash, effects)?;
    let ExecutionResultAndMessages {
        execution_result,
        messages,
    } = ExecutionResultAndMessages::from(ee_execution_result);
    let versioned_execution_result = ExecutionResult::from(execution_result);
    Ok((new_state_root, versioned_execution_result, messages))
}

fn commit_transforms(
    scratch_state: &ScratchGlobalState,
    metrics: Option<Arc<Metrics>>,
    state_root_hash: Digest,
    effects: Effects,
) -> Result<Digest, GlobalStateError> {
    trace!(?state_root_hash, ?effects, "commit");
    let start = Instant::now();
    let result = scratch_state.commit(state_root_hash, effects);
    if let Some(metrics) = metrics {
        metrics.apply_effect.observe(start.elapsed().as_secs_f64());
    }
    trace!(?result, "commit result");
    result
}

/// Execute the transaction without committing the effects.
/// Intended to be used for discovery operations on read-only nodes.
///
/// Returns effects of the execution.
pub fn speculatively_execute<S>(
    state_provider: &S,
    execution_engine_v1: &ExecutionEngineV1,
    execution_state: SpeculativeExecutionState,
    deploy: DeployItem,
) -> Result<SpeculativeExecutionResult, engine_state::Error>
where
    S: StateProvider,
{
    let SpeculativeExecutionState {
        state_root_hash,
        block_time,
        protocol_version,
    } = execution_state;
    let deploy_hash = deploy.deploy_hash;
    let execute_request = ExecuteRequest::new(
        state_root_hash,
        block_time.millis(),
        vec![deploy],
        protocol_version,
        PublicKey::System,
    );
    let results = execute(state_provider, execution_engine_v1, None, execute_request);
    results.map(|mut execution_results| {
        let len = execution_results.len();
        if len != 1 {
            warn!(
                ?deploy_hash,
                "got more ({}) execution results from a single transaction", len
            );
            SpeculativeExecutionResult::new(None)
        } else {
            // We know it must be 1, we could unwrap and then wrap
            // with `Some(_)` but `pop_front` already returns an `Option`.
            // We need to transform the `engine_state::ExecutionResult` into
            // `casper_types::ExecutionResult` as well.
            SpeculativeExecutionResult::new(execution_results.pop_front().map(|result| {
                let ExecutionResultAndMessages {
                    execution_result,
                    messages,
                } = result.into();

                (execution_result, messages)
            }))
        }
    })
}

fn execute<S>(
    state_provider: &S,
    execution_engine_v1: &ExecutionEngineV1,
    metrics: Option<Arc<Metrics>>,
    execute_request: ExecuteRequest,
) -> Result<ExecutionResults, engine_state::Error>
where
    S: StateProvider,
{
    trace!(?execute_request, "execute");
    let start = Instant::now();
    let result = execution_engine_v1.exec(state_provider, execute_request);
    if let Some(metrics) = metrics {
        metrics.run_execute.observe(start.elapsed().as_secs_f64());
    }
    trace!(?result, "execute result");
    result
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
