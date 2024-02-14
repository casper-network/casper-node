use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{
    self, execution_result::ExecutionResultAndMessages, step::EvictItem, EngineState,
    ExecuteRequest, ExecutionResult as EngineExecutionResult, PruneConfig, PruneResult, StepError,
    StepRequest, StepSuccess,
};
use casper_storage::{
    data_access_layer::{
        transfer::TransferConfig, DataAccessLayer, EraValidatorsRequest, EraValidatorsResult,
        TransferRequest,
    },
    global_state::state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider, StateReader},
};
// use casper_storage::global_state::error::Error as GlobalStateError;
use casper_types::{
    bytesrepr::{self, ToBytes, U32_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::{Effects, ExecutionResult, ExecutionResultV2, Transform, TransformKind},
    BlockTime, BlockV2, CLValue, ChecksumRegistry, Digest, EraEndV2, EraId, Gas, Key,
    ProtocolVersion, PublicKey, Transaction, TransactionHash, TransactionHeader, U512,
};

use crate::{
    components::{
        contract_runtime::{
            error::BlockExecutionError, types::StepEffectsAndUpcomingEraValidators,
            BlockAndExecutionResults, ExecutionPreState, Metrics, SpeculativeExecutionError,
            SpeculativeExecutionState, APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
        },
        fetcher::FetchItem,
    },
    contract_runtime::utils::calculate_prune_eras,
    types::{self, ApprovalsHashes, Chunkable, ExecutableBlock, InternalEraReport},
};

use super::ExecutionArtifact;

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    engine_state: &EngineState<DataAccessLayer<LmdbGlobalState>>,
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
    metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    execution_pre_state: ExecutionPreState,
    executable_block: ExecutableBlock,
    activation_point_era_id: EraId,
    key_block_height_for_activation_point: u64,
    prune_batch_size: u64,
) -> Result<BlockAndExecutionResults, BlockExecutionError> {
    if executable_block.height != execution_pre_state.next_block_height() {
        return Err(BlockExecutionError::WrongBlockHeight {
            executable_block: Box::new(executable_block),
            execution_pre_state: Box::new(execution_pre_state),
        });
    }

    let pre_state_root_hash = execution_pre_state.pre_state_root_hash();
    let parent_hash = execution_pre_state.parent_hash();
    let parent_seed = execution_pre_state.parent_seed();
    let next_block_height = execution_pre_state.next_block_height();

    let mut state_root_hash = pre_state_root_hash;
    let mut execution_results: Vec<ExecutionArtifact> =
        Vec::with_capacity(executable_block.transactions.len());
    // Run any transactions that must be executed
    let block_time = executable_block.timestamp.millis();
    let start = Instant::now();
    let txn_ids = executable_block
        .transactions
        .iter()
        .map(Transaction::fetch_id)
        .collect_vec();
    let approvals_checksum = types::compute_approvals_checksum(txn_ids.clone())
        .map_err(BlockExecutionError::FailedToComputeApprovalsChecksum)?;

    // Create a new EngineState that reads from LMDB but only caches changes in memory.
    let scratch_state = engine_state.get_scratch_engine_state();
    // let scratch_state = data_access_layer.get_scratch_engine_state();

    // Pay out block rewards
    if let Some(rewards) = &executable_block.rewards {
        state_root_hash = scratch_state.distribute_block_rewards(
            state_root_hash,
            protocol_version,
            rewards,
            next_block_height,
            block_time,
        )?;
    }

    for transaction in executable_block.transactions {
        let (txn_hash, txn) = match transaction {
            Transaction::Deploy(deploy) => {
                let deploy_hash = *deploy.hash();
                let txn_hash = TransactionHash::Deploy(deploy_hash);
                if deploy.is_transfer() {
                    // native transfers are routed to the data provider
                    let authorization_keys = deploy
                        .approvals()
                        .iter()
                        .map(|approval| approval.signer().to_account_hash())
                        .collect();
                    let transfer_req = TransferRequest::with_runtime_args(
                        TransferConfig::Unadministered, /* TODO: check chainspec & handle
                                                         * administered possibility */
                        state_root_hash,
                        block_time,
                        protocol_version,
                        PublicKey::clone(&executable_block.proposer),
                        deploy_hash.into(),
                        deploy.header().account().to_account_hash(),
                        authorization_keys,
                        deploy.session().args().clone(),
                        U512::zero(), /* <-- this should be the native transfer cost from the
                                       * chainspec */
                    );
                    // native transfer auto-commits
                    let transfer_result = data_access_layer.transfer(transfer_req);
                    trace!(%txn_hash, ?transfer_result, "native transfer result");
                    match EngineExecutionResult::from_transfer_result(transfer_result, Gas::zero())
                    {
                        Err(_) => return Err(BlockExecutionError::RootNotFound(state_root_hash)),
                        Ok(exec_result) => {
                            let ExecutionResultAndMessages {
                                execution_result,
                                messages,
                            } = ExecutionResultAndMessages::from(exec_result);
                            let versioned_execution_result =
                                ExecutionResult::from(execution_result);
                            execution_results.push(ExecutionArtifact::new(
                                txn_hash,
                                TransactionHeader::Deploy(deploy.header().clone()),
                                versioned_execution_result,
                                messages,
                            ));
                        }
                    }
                    continue;
                }
                (
                    TransactionHash::Deploy(deploy_hash),
                    Transaction::Deploy(deploy),
                )
            }
            txn @ Transaction::V1(_) => (txn.hash(), txn),
        };

        if txn.is_native() {
            todo!("route native transactions to data access layer");
        }

        let txn_header = match &txn {
            Transaction::Deploy(deploy) => TransactionHeader::from(deploy.header().clone()),
            Transaction::V1(v1_txn) => TransactionHeader::from(v1_txn.header().clone()),
        };
        let execute_request = ExecuteRequest::new(
            state_root_hash,
            BlockTime::new(block_time),
            txn,
            (*executable_block.proposer).clone(),
        )?;

        let exec_result_and_msgs = execute(&scratch_state, metrics.clone(), execute_request)?;

        trace!(%txn_hash, ?exec_result_and_msgs, "transaction execution result");
        // As for now a given state is expected to exist.
        let new_state_root_hash = commit_execution_result(
            &scratch_state,
            // data_access_layer,
            metrics.clone(),
            state_root_hash,
            txn_hash,
            &exec_result_and_msgs.execution_result,
        )?;
        execution_results.push(ExecutionArtifact::new(
            txn_hash,
            txn_header,
            ExecutionResult::from(exec_result_and_msgs.execution_result),
            exec_result_and_msgs.messages,
        ));
        state_root_hash = new_state_root_hash;
    }

    // Write the transaction approvals' and execution results' checksums to global state.
    let execution_results_checksum = compute_execution_results_checksum(
        execution_results
            .iter()
            .map(|artifact| &artifact.execution_result),
    )?;

    let mut checksum_registry = ChecksumRegistry::new();
    checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);
    checksum_registry.insert(EXECUTION_RESULTS_CHECKSUM_NAME, execution_results_checksum);
    let mut effects = Effects::new();
    effects.push(Transform::new(
        Key::ChecksumRegistry,
        TransformKind::Write(
            CLValue::from_t(checksum_registry)
                .map_err(BlockExecutionError::ChecksumRegistryToCLValue)?
                .into(),
        ),
    ));
    scratch_state.commit_effects(state_root_hash, effects)?;

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // If the finalized block has an era report, run the auction contract and get the upcoming era
    // validators.
    let maybe_step_effects_and_upcoming_era_validators = if let Some(era_report) =
        &executable_block.era_report
    {
        let StepSuccess {
            post_state_hash: _, // ignore the post-state-hash returned from scratch
            effects: step_effects,
        } = commit_step(
            &scratch_state, // engine_state
            metrics,
            protocol_version,
            state_root_hash,
            era_report.clone(),
            executable_block.timestamp.millis(),
            executable_block.era_id.successor(),
        )?;

        state_root_hash =
            engine_state.write_scratch_to_db(state_root_hash, scratch_state.into_inner())?;

        // state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;

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
        state_root_hash =
            engine_state.write_scratch_to_db(state_root_hash, scratch_state.into_inner())?;
        // state_root_hash = data_access_layer.write_scratch_to_db(state_root_hash, scratch_state)?;
        None
    };

    // Flush once, after all deploys have been executed.
    engine_state.flush_environment()?;

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
            let prune_config = PruneConfig::new(state_root_hash, keys_to_prune);
            match engine_state.commit_prune(prune_config) {
                Ok(PruneResult::RootNotFound) => {
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
                Ok(PruneResult::DoesNotExist) => {
                    warn!(
                        previous_block_height,
                        %state_root_hash,
                        "commit prune: key does not exist"
                    );
                }
                Ok(PruneResult::Success {
                    post_state_hash, ..
                }) => {
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
                Err(error) => {
                    error!(
                        previous_block_height,
                        %key_block_height_for_activation_point,
                        %error,
                        "commit prune: commit prune error"
                    );
                    return Err(error.into());
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
            return Err(BlockExecutionError::FailedToCreateEraEnd {
                maybe_era_report,
                maybe_next_era_validator_weights,
            })
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

    let approvals_hashes = txn_ids.into_iter().map(|id| id.approvals_hash()).collect();

    let proof_of_checksum_registry = match data_access_layer.tracking_copy(state_root_hash)? {
        Some(tc) => match tc.reader().read_with_proof(&Key::ChecksumRegistry)? {
            Some(proof) => proof,
            None => {
                return Err(BlockExecutionError::EngineState(
                    engine_state::Error::MissingChecksumRegistry,
                ))
            }
        },
        None => {
            return Err(BlockExecutionError::EngineState(
                engine_state::Error::RootNotFound(state_root_hash),
            ))
        }
    };

    let approvals_hashes = Box::new(ApprovalsHashes::new_v2(
        *block.hash(),
        approvals_hashes,
        proof_of_checksum_registry,
    ));

    Ok(BlockAndExecutionResults {
        block,
        approvals_hashes,
        execution_results,
        maybe_step_effects_and_upcoming_era_validators,
    })
}

/// Commits the execution results.
fn commit_execution_result<S>(
    engine_state: &EngineState<S>,
    // data_access_layer: &DataAccessLayer<S>,
    metrics: Option<Arc<Metrics>>,
    state_root_hash: Digest,
    txn_hash: TransactionHash,
    execution_result: &ExecutionResultV2,
) -> Result<Digest, BlockExecutionError>
where
    S: StateProvider + CommitProvider,
{
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
    let commit_result = engine_state.commit_effects(state_root_hash, effects);
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
    engine_state: &EngineState<S>,
    execution_state: SpeculativeExecutionState,
    txn: Transaction,
) -> Result<(ExecutionResultV2, Messages), SpeculativeExecutionError>
where
    S: StateProvider + CommitProvider,
{
    let SpeculativeExecutionState {
        state_root_hash,
        block_time,
        protocol_version: _,
    } = execution_state;
    if txn.is_native() {
        todo!("route native transactions to data access layer");
    }
    let execute_request = ExecuteRequest::new(
        state_root_hash,
        BlockTime::new(block_time.millis()),
        txn,
        PublicKey::System,
    )?;
    execute(engine_state, None, execute_request)
        .map(|res_and_msgs| (res_and_msgs.execution_result, res_and_msgs.messages))
        .map_err(SpeculativeExecutionError::from)
}

fn execute<S>(
    engine_state: &EngineState<S>,
    metrics: Option<Arc<Metrics>>,
    execute_request: ExecuteRequest,
) -> Result<ExecutionResultAndMessages, engine_state::Error>
where
    S: StateProvider + CommitProvider,
{
    trace!(?execute_request, "execute");
    let start = Instant::now();
    let result = engine_state
        .execute_transaction(execute_request)
        .map(ExecutionResultAndMessages::from);
    if let Some(metrics) = metrics {
        metrics.run_execute.observe(start.elapsed().as_secs_f64());
    }
    trace!(?result, "execute result");
    result
}

fn commit_step<S>(
    engine_state: &EngineState<S>,
    maybe_metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    pre_state_root_hash: Digest,
    InternalEraReport {
        equivocators,
        inactive_validators,
    }: InternalEraReport,
    era_end_timestamp_millis: u64,
    next_era_id: EraId,
) -> Result<StepSuccess, StepError>
where
    S: StateProvider + CommitProvider,
{
    // Both inactive validators and equivocators are evicted
    let evict_items = inactive_validators
        .into_iter()
        .chain(equivocators)
        .map(EvictItem::new)
        .collect();

    let step_request = StepRequest {
        pre_state_hash: pre_state_root_hash,
        protocol_version,
        // Note: The Casper Network does not slash, but another network could
        slash_items: vec![],
        evict_items,
        next_era_id,
        era_end_timestamp_millis,
    };

    // Have the EE commit the step.
    let start = Instant::now();
    let result = engine_state.commit_step(step_request);
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
