use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::engine_state::{
    self,
    execution_result::{ExecutionResultAndMessages, ExecutionResults},
    DeployItem, EngineState, ExecuteRequest, ExecutionResult as EngineExecutionResult,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        AuctionMethod, BiddingRequest, BiddingResult, BlockRewardsRequest, BlockRewardsResult,
        DataAccessLayer, EraValidatorsRequest, EraValidatorsResult, EvictItem, FeeRequest,
        FeeResult, FlushRequest, PruneRequest, PruneResult, StepRequest, StepResult,
        TransferRequest, TransferResult,
    },
    global_state::{
        error::Error as GlobalStateError,
        state::{
            lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, StateProvider,
            StateReader,
        },
    },
    system::runtime_native::Config as NativeRuntimeConfig,
};
use casper_types::{
    binary_port::SpeculativeExecutionResult,
    bytesrepr::{self, ToBytes, U32_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::{Effects, ExecutionResult, ExecutionResultV2, Transform, TransformKind},
    BlockV2, CLValue, Chainspec, ChecksumRegistry, DeployHash, Digest, EraEndV2, EraId, Key,
    ProtocolVersion, PublicKey, Transaction, TransactionApprovalsHash, U512,
};

use crate::{
    components::{
        contract_runtime::{
            error::BlockExecutionError, types::StepEffectsAndUpcomingEraValidators,
            BlockAndExecutionResults, ExecutionPreState, Metrics, SpeculativeExecutionState,
            APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
        },
        fetcher::FetchItem,
    },
    contract_runtime::utils::calculate_prune_eras,
    types::{self, Chunkable, ExecutableBlock, InternalEraReport},
};

use super::ExecutionArtifact;

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    engine_state: &EngineState<DataAccessLayer<LmdbGlobalState>>,
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
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
    let native_runtime_config = NativeRuntimeConfig::from_chainspec(chainspec);

    let pre_state_root_hash = execution_pre_state.pre_state_root_hash();
    let parent_hash = execution_pre_state.parent_hash();
    let parent_seed = execution_pre_state.parent_seed();

    let mut state_root_hash = pre_state_root_hash;
    let mut execution_artifacts: Vec<ExecutionArtifact> =
        Vec::with_capacity(executable_block.transactions.len());
    let block_time = executable_block.timestamp.millis();
    let start = Instant::now();
    let txn_ids = executable_block
        .transactions
        .iter()
        .map(Transaction::fetch_id)
        .collect_vec();
    let approvals_checksum = types::compute_approvals_checksum(txn_ids.clone())
        .map_err(BlockExecutionError::FailedToComputeApprovalsChecksum)?;
    let approvals_hashes: Vec<TransactionApprovalsHash> =
        txn_ids.into_iter().map(|id| id.approvals_hash()).collect();

    let scratch_state = data_access_layer.get_scratch_engine_state();
    let mut effects = Effects::new();

    // Pay out fees, if relevant.
    {
        let fee_req = FeeRequest::new(
            native_runtime_config.clone(),
            state_root_hash,
            protocol_version,
            block_time,
        );
        match data_access_layer.distribute_fees(fee_req) {
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
        match data_access_layer.distribute_block_rewards(rewards_req) {
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

    for transaction in executable_block.transactions {
        let transaction_hash = transaction.hash();
        if transaction.is_native_mint() {
            // native transfers are routed to the data provider
            let authorization_keys = transaction.authorization_keys();
            let transfer_req = TransferRequest::with_runtime_args(
                native_runtime_config.clone(),
                state_root_hash,
                block_time,
                protocol_version,
                transaction_hash,
                transaction.initiator_addr().account_hash(),
                authorization_keys,
                transaction.session_args().clone(),
                U512::zero(), /* <-- this should be from chainspec cost table */
            );
            //NOTE: native mint interactions auto-commit
            let transfer_result = data_access_layer.transfer(transfer_req);
            trace!(
                ?transaction_hash,
                ?transfer_result,
                "native transfer result"
            );
            match transfer_result {
                TransferResult::RootNotFound => {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash));
                }
                TransferResult::Failure(transfer_error) => {
                    let artifact = ExecutionArtifact::new(
                        transaction_hash,
                        transaction.header(),
                        ExecutionResult::V2(ExecutionResultV2::Failure {
                            effects: Effects::new(),
                            cost: U512::zero(),
                            transfers: vec![],
                            error_message: format!("{:?}", transfer_error),
                        }),
                        Messages::default(),
                    );
                    execution_artifacts.push(artifact);
                    debug!(%transfer_error);
                    // a failure does not auto commit
                    continue;
                }
                TransferResult::Success {
                    post_state_hash,
                    transfers,
                    ..
                } => {
                    // ideally we should be doing something with the transfer records and effects,
                    // but currently this is auto-commit and if we include the effects in the
                    // collection they will get double-committed. there's also
                    // the problem that the execution results type needs to be
                    // made more expressive / flexible. effects? transfers?
                    state_root_hash = post_state_hash;
                    let artifact = ExecutionArtifact::new(
                        transaction_hash,
                        transaction.header(),
                        ExecutionResult::V2(ExecutionResultV2::Success {
                            effects: Effects::new(), // auto commit
                            cost: U512::zero(),
                            transfers,
                        }),
                        Messages::default(),
                    );
                    execution_artifacts.push(artifact);
                }
            }
            continue;
        }
        if transaction.is_native_auction() {
            let args = transaction.session_args();
            let entry_point = transaction.entry_point();
            let auction_method = match AuctionMethod::from_parts(entry_point, args, chainspec) {
                Ok(auction_method) => auction_method,
                Err(_) => {
                    error!(%transaction_hash, "failed to resolve auction method");
                    continue; // skip to next record
                }
            };
            let authorization_keys = transaction.authorization_keys();
            let bidding_req = BiddingRequest::new(
                native_runtime_config.clone(),
                state_root_hash,
                block_time,
                protocol_version,
                transaction_hash,
                transaction.initiator_addr().account_hash(),
                authorization_keys,
                auction_method,
            );

            //NOTE: native mint interactions auto-commit
            let bidding_result = data_access_layer.bidding(bidding_req);
            trace!(?transaction_hash, ?bidding_result, "native auction result");
            match bidding_result {
                BiddingResult::RootNotFound => {
                    return Err(BlockExecutionError::RootNotFound(state_root_hash))
                }
                BiddingResult::Success {
                    post_state_hash, ..
                } => {
                    // we need a way to capture the effects from this without double committing
                    state_root_hash = post_state_hash;
                }
                BiddingResult::Failure(tce) => {
                    debug!(%tce);
                    continue;
                }
            }
        }

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
            vec![DeployItem::from(deploy)],
            protocol_version,
            PublicKey::clone(&executable_block.proposer),
        );

        let exec_result = execute(
            engine_state,
            data_access_layer,
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
        execution_artifacts.push(ExecutionArtifact::deploy(
            deploy_hash,
            deploy_header,
            execution_result,
            messages,
        ));
        state_root_hash = state_hash;
    }

    // handle checksum registry
    let approvals_hashes = {
        let mut checksum_registry = ChecksumRegistry::new();

        checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);

        // Write the deploy approvals' and execution results' checksums to global state.
        let execution_results_checksum = compute_execution_results_checksum(
            execution_artifacts
                .iter()
                .map(|artifact| &artifact.execution_result),
        )?;
        checksum_registry.insert(EXECUTION_RESULTS_CHECKSUM_NAME, execution_results_checksum);

        effects.push(Transform::new(
            Key::ChecksumRegistry,
            TransformKind::Write(
                CLValue::from_t(checksum_registry)
                    .map_err(BlockExecutionError::ChecksumRegistryToCLValue)?
                    .into(),
            ),
        ));

        approvals_hashes
    };

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

    // Flush once, after all deploys have been executed.
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

        Box::new(ApprovalsHashes::new_v2(
            *block.hash(),
            approvals_hashes,
            proof_of_checksum_registry,
        ))
    };

    Ok(BlockAndExecutionResults {
        block,
        approvals_hashes,
        execution_results: execution_artifacts,
        maybe_step_effects_and_upcoming_era_validators,
    })
}

/// Commits the execution results.
fn commit_execution_results(
    //data_access_layer: &DataAccessLayer<S>,
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
    // data_access_layer: &DataAccessLayer<S>,
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
    engine_state: &EngineState<S>,
    data_access_layer: &S,
    execution_state: SpeculativeExecutionState,
    deploy: DeployItem,
) -> Result<SpeculativeExecutionResult, engine_state::Error>
where
    S: StateProvider + CommitProvider,
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
    let results = execute(engine_state, data_access_layer, None, execute_request);
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
    engine_state: &EngineState<S>,
    _data_access_layer: &S,
    metrics: Option<Arc<Metrics>>,
    execute_request: ExecuteRequest,
) -> Result<ExecutionResults, engine_state::Error>
where
    S: StateProvider + CommitProvider,
{
    trace!(?execute_request, "execute");
    let start = Instant::now();
    let result = engine_state.run_execute(execute_request);
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
