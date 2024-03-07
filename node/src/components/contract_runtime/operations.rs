use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{
    self, execution_result::ExecutionResultAndMessages, ExecuteRequest, ExecutionEngineV1,
    ExecutionResult as EngineExecutionResult,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        BlockRewardsRequest, BlockRewardsResult, DataAccessLayer, EraValidatorsRequest,
        EraValidatorsResult, EvictItem, FeeRequest, FeeResult, FlushRequest, PruneRequest,
        PruneResult, StepRequest, StepResult, TransferRequest,
    },
    global_state::state::{
        lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, ScratchProvider,
        StateProvider, StateReader,
    },
    system::runtime_native::Config as NativeRuntimeConfig,
};
use casper_types::{
    binary_port::SpeculativeExecutionResult,
    bytesrepr::{self, ToBytes, U32_SERIALIZED_LENGTH},
    execution::{Effects, ExecutionResult, ExecutionResultV2, Transform, TransformKind},
    BlockTime, BlockV2, CLValue, Chainspec, ChecksumRegistry, Digest, EraEndV2, EraId, Gas, Key,
    ProtocolVersion, PublicKey, SystemConfig, Transaction, TransactionApprovalsHash,
    TransactionEntryPoint, TransactionHash, TransactionHeader, U512,
};

use super::{
    utils::calculate_prune_eras, BlockAndExecutionResults, BlockExecutionError, ExecutionArtifact,
    ExecutionPreState, Metrics, NewUserRequestError, SpeculativeExecutionError,
    SpeculativeExecutionState, StepEffectsAndUpcomingEraValidators, APPROVALS_CHECKSUM_NAME,
    EXECUTION_RESULTS_CHECKSUM_NAME,
};
use crate::{
    components::fetcher::FetchItem,
    types::{self, Chunkable, ExecutableBlock, InternalEraReport},
};

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
) -> Result<BlockAndExecutionResults, BlockExecutionError> {
    if executable_block.height != execution_pre_state.next_block_height() {
        return Err(BlockExecutionError::WrongBlockHeight {
            executable_block: Box::new(executable_block),
            execution_pre_state: Box::new(execution_pre_state),
        });
    }

    let protocol_version = chainspec.protocol_version();
    let activation_point_era_id = chainspec.protocol_config.activation_point.era_id();
    let prune_batch_size = chainspec.core_config.prune_batch_size;
    let system_costs = chainspec.system_costs_config;
    let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);

    let pre_state_root_hash = execution_pre_state.pre_state_root_hash();
    let parent_hash = execution_pre_state.parent_hash();
    let parent_seed = execution_pre_state.parent_seed();

    let mut state_root_hash = pre_state_root_hash;
    let mut execution_artifacts: Vec<ExecutionArtifact> =
        Vec::with_capacity(executable_block.transactions.len());
    // Run any transactions that must be executed
    let block_time = BlockTime::new(executable_block.timestamp.millis());
    let start = Instant::now();
    let txn_ids = executable_block
        .transactions
        .iter()
        .map(Transaction::fetch_id)
        .collect_vec();
    let approvals_checksum = types::compute_approvals_checksum(txn_ids.clone())
        .map_err(BlockExecutionError::FailedToComputeApprovalsChecksum)?;

    let scratch_state = data_access_layer.get_scratch_global_state();
    let mut effects = Effects::new();

    // Pay out fees, if relevant.
    {
        let fee_req = FeeRequest::new(
            native_runtime_config.clone(),
            state_root_hash,
            protocol_version,
            block_time,
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

    // Pay out  ̶b̶l̶o̶c̶k̶ e͇r͇a͇ rewards
    // NOTE: despite the name, these rewards are currently paid out per ERA not per BLOCK
    // at one point, they were going to be paid out per block (and might be in the future)
    // but it ended up settling on per era. the behavior is driven by Some / None as sent
    // thus if in future calling logic passes rewards per block it should just work as is.
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

    for txn in executable_block.transactions {
        let txn_hash = txn.hash();
        let txn_header = match &txn {
            Transaction::Deploy(deploy) => TransactionHeader::from(deploy.header().clone()),
            Transaction::V1(v1_txn) => TransactionHeader::from(v1_txn.header().clone()),
        };

        let request = UserRequest::new(
            state_root_hash,
            block_time,
            protocol_version,
            txn,
            (*executable_block.proposer).clone(),
            native_runtime_config.clone(),
            &system_costs,
        )?;

        match request {
            UserRequest::Execute(execute_request) => {
                let exec_result_and_msgs = execute(
                    &scratch_state,
                    execution_engine_v1,
                    metrics.clone(),
                    execute_request,
                )?;

                trace!(%txn_hash, ?exec_result_and_msgs, "transaction execution result");
                // As for now a given state is expected to exist.
                let new_state_root_hash = commit_execution_result(
                    &scratch_state,
                    metrics.clone(),
                    state_root_hash,
                    txn_hash,
                    &exec_result_and_msgs.execution_result,
                )?;
                execution_artifacts.push(ExecutionArtifact::new(
                    txn_hash,
                    txn_header,
                    ExecutionResult::from(exec_result_and_msgs.execution_result),
                    exec_result_and_msgs.messages,
                ));
                state_root_hash = new_state_root_hash;
            }
            UserRequest::Transfer(transfer_request) => {
                let gas = transfer_request.gas();
                // native transfer auto-commits
                let transfer_result = data_access_layer.transfer(transfer_request);
                // if let TransferResult::Success {
                //     post_state_hash, ..
                // } = &transfer_result
                // {
                //     state_root_hash = *post_state_hash;
                // }
                trace!(%txn_hash, ?transfer_result, "native transfer result");
                let Ok(exec_result) = EngineExecutionResult::from_transfer_result(transfer_result, gas) else {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                };
                let exec_result_and_msgs = ExecutionResultAndMessages::from(exec_result);
                execution_artifacts.push(ExecutionArtifact::new(
                    txn_hash,
                    txn_header,
                    ExecutionResult::from(exec_result_and_msgs.execution_result),
                    exec_result_and_msgs.messages,
                ));
            }
        }
    }

    // Write the transaction approvals' and execution results' checksums to global state.
    let execution_results_checksum = compute_execution_results_checksum(
        execution_artifacts
            .iter()
            .map(|artifact| &artifact.execution_result),
    )?;

    // handle checksum registry
    let mut checksum_registry = ChecksumRegistry::new();
    checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);
    checksum_registry.insert(EXECUTION_RESULTS_CHECKSUM_NAME, execution_results_checksum);
    effects.push(Transform::new(
        Key::ChecksumRegistry,
        TransformKind::Write(
            CLValue::from_t(checksum_registry)
                .map_err(BlockExecutionError::ChecksumRegistryToCLValue)?
                .into(),
        ),
    ));
    scratch_state.commit(state_root_hash, effects)?;

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // If the finalized block has an era report, run the auction contract and get the upcoming era
    // validators.
    let maybe_step_effects_and_upcoming_era_validators = if let Some(era_report) =
        &executable_block.era_report
    {
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
            StepResult::Success { effects, .. } => effects,
        };

        state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;

        let era_validators_req = EraValidatorsRequest::new(state_root_hash, protocol_version);
        let era_validators_result = data_access_layer.era_validators(era_validators_req);

        let upcoming_era_validators = match era_validators_result {
            EraValidatorsResult::AuctionNotFound => {
                panic!("auction not found");
            }
            EraValidatorsResult::RootNotFound => {
                panic!("root not found");
            }
            EraValidatorsResult::ValueNotFound(msg) => {
                panic!("validator snapshot not found: {}", msg);
            }
            EraValidatorsResult::Failure(tce) => {
                return Err(BlockExecutionError::GetEraValidators(tce));
            }
            EraValidatorsResult::Success { era_validators } => era_validators,
        };

        Some(StepEffectsAndUpcomingEraValidators {
            step_effects,
            upcoming_era_validators,
        })
    } else {
        // Finally, the new state-root-hash from the cumulative changes to global state is
        // returned when they are written to LMDB.
        state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;
        None
    };

    // Flush once, after all transactions have been executed.
    let flush_req = FlushRequest::new();
    let flush_result = data_access_layer.flush(flush_req);
    if let Err(gse) = flush_result.as_error() {
        error!("failed to flush lmdb");
        return Err(BlockExecutionError::Lmdb(gse));
    }

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

    let maybe_next_era_validator_weights: Option<BTreeMap<PublicKey, U512>> = {
        let next_era_id = executable_block.era_id.successor();
        maybe_step_effects_and_upcoming_era_validators
            .as_ref()
            .and_then(
                |StepEffectsAndUpcomingEraValidators {
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
        executable_block.transfer,
        executable_block.staking,
        executable_block.install_upgrade,
        executable_block.standard,
        executable_block.rewarded_signatures,
    ));

    let approvals_hashes: Vec<TransactionApprovalsHash> =
        txn_ids.into_iter().map(|id| id.approvals_hash()).collect();

    let proof_of_checksum_registry = match data_access_layer.tracking_copy(state_root_hash)? {
        Some(tc) => match tc.reader().read_with_proof(&Key::ChecksumRegistry)? {
            Some(proof) => proof,
            None => return Err(BlockExecutionError::MissingChecksumRegistry),
        },
        None => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
    };

    let approvals_hashes = Box::new(ApprovalsHashes::new_v2(
        *block.hash(),
        approvals_hashes,
        proof_of_checksum_registry,
    ));

    Ok(BlockAndExecutionResults {
        block,
        approvals_hashes,
        execution_results: execution_artifacts,
        maybe_step_effects_and_upcoming_era_validators,
    })
}

/// Commits the execution results.
fn commit_execution_result(
    scratch_state: &ScratchGlobalState,
    metrics: Option<Arc<Metrics>>,
    state_root_hash: Digest,
    txn_hash: TransactionHash,
    execution_result: &ExecutionResultV2,
) -> Result<Digest, BlockExecutionError> {
    let start = Instant::now();
    let effects = match execution_result {
        ExecutionResultV2::Success { effects, gas, .. } => {
            debug!(%txn_hash, %gas, "execution succeeded");
            effects.clone()
        }
        ExecutionResultV2::Failure {
            error_message,
            effects,
            gas,
            ..
        } => {
            debug!(%txn_hash, %error_message, %gas, "execution failed");
            effects.clone()
        }
    };
    let commit_result = scratch_state.commit(state_root_hash, effects);
    if let Some(metrics) = metrics {
        metrics.apply_effect.observe(start.elapsed().as_secs_f64());
    }
    commit_result.map_err(BlockExecutionError::from)
}

/// Execute the transaction without committing the effects.
/// Intended to be used for discovery operations on read-only nodes.
///
/// Returns effects of the execution.
pub(super) fn speculatively_execute<S>(
    state_provider: &S,
    execution_engine_v1: &ExecutionEngineV1,
    execution_state: SpeculativeExecutionState,
    native_runtime_config: NativeRuntimeConfig,
    system_costs: SystemConfig,
    txn: Transaction,
) -> Result<SpeculativeExecutionResult, SpeculativeExecutionError>
where
    S: StateProvider,
{
    let SpeculativeExecutionState {
        state_root_hash,
        block_time,
        protocol_version,
    } = execution_state;

    let request = UserRequest::new(
        state_root_hash,
        BlockTime::new(block_time.millis()),
        protocol_version,
        txn,
        PublicKey::System,
        native_runtime_config,
        &system_costs,
    )?;

    match request {
        UserRequest::Execute(execute_request) => {
            execute(state_provider, execution_engine_v1, None, execute_request)
                .map(|res_and_msgs| {
                    SpeculativeExecutionResult::new(
                        res_and_msgs.execution_result,
                        res_and_msgs.messages,
                    )
                })
                .map_err(SpeculativeExecutionError::from)
        }
        UserRequest::Transfer(_transfer_request) => {
            todo!("route native transactions to data access layer, but don't commit them");
        }
    }
}

fn execute<S>(
    state_provider: &S,
    execution_engine_v1: &ExecutionEngineV1,
    metrics: Option<Arc<Metrics>>,
    execute_request: ExecuteRequest,
) -> Result<ExecutionResultAndMessages, engine_state::Error>
where
    S: StateProvider,
{
    trace!(?execute_request, "execute");
    let start = Instant::now();
    let result = execution_engine_v1
        .exec(state_provider, execute_request)
        .map(ExecutionResultAndMessages::from);
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

enum UserRequest {
    Execute(ExecuteRequest),
    Transfer(TransferRequest),
}

impl UserRequest {
    fn new(
        state_hash: Digest,
        block_time: BlockTime,
        protocol_version: ProtocolVersion,
        txn: Transaction,
        proposer: PublicKey,
        native_runtime_config: NativeRuntimeConfig,
        system_costs: &SystemConfig,
    ) -> Result<Self, NewUserRequestError> {
        if txn.is_native_mint() {
            let txn_hash = txn.hash();
            let initiator_addr = txn.initiator_addr();
            let authorization_keys = txn.signers();

            let v1_txn = match txn {
                Transaction::Deploy(deploy) => {
                    if !deploy.is_transfer() {
                        return Err(NewUserRequestError::ExpectedNativeTransferDeploy(txn_hash));
                    }
                    let transfer_req = TransferRequest::with_runtime_args(
                        native_runtime_config,
                        state_hash,
                        block_time,
                        protocol_version,
                        txn_hash,
                        initiator_addr,
                        authorization_keys,
                        deploy.session().args().clone(),
                        Gas::new(system_costs.mint_costs().transfer),
                    );
                    return Ok(UserRequest::Transfer(transfer_req));
                }
                Transaction::V1(v1_txn) => v1_txn,
            };

            match v1_txn.entry_point() {
                TransactionEntryPoint::Custom(_) => {
                    return Err(NewUserRequestError::InvalidEntryPoint(txn_hash));
                }
                TransactionEntryPoint::Transfer => {
                    let transfer_req = TransferRequest::with_runtime_args(
                        native_runtime_config,
                        state_hash,
                        block_time,
                        protocol_version,
                        txn_hash,
                        initiator_addr,
                        authorization_keys,
                        v1_txn.take_args(),
                        Gas::new(system_costs.mint_costs().transfer),
                    );
                    return Ok(UserRequest::Transfer(transfer_req));
                }
                TransactionEntryPoint::AddBid => todo!("make auction request"),
                TransactionEntryPoint::WithdrawBid => todo!("make auction request"),
                TransactionEntryPoint::Delegate => todo!("make auction request"),
                TransactionEntryPoint::Undelegate => todo!("make auction request"),
                TransactionEntryPoint::Redelegate => todo!("make auction request"),
                TransactionEntryPoint::ActivateBid => todo!("make auction request"),
            }
        }

        let execute_req = ExecuteRequest::new(state_hash, block_time, txn, proposer)
            .map_err(NewUserRequestError::Execute)?;
        Ok(UserRequest::Execute(execute_req))
    }
}
