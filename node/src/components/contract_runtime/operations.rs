pub(crate) mod wasm_v2_request;

use casper_executor_wasm::ExecutorV2;
use itertools::Itertools;
use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Instant};
use tracing::{debug, error, info, trace, warn};
use wasm_v2_request::{WasmV2Request, WasmV2Result};

use casper_execution_engine::engine_state::{
    BlockInfo, ExecutionEngineV1, WasmV1Request, WasmV1Result,
};
use casper_executor_evm::{
    BlockContext as EvmBlockContext, BlockHashProvider as EvmBlockHashProvider,
    BlockHashProviderResult as EvmBlockHashProviderResult, CallRequest as EvmExecutorCallRequest,
    CallValidation as EvmCallValidation, EvmExecutor, ExecuteKind as EvmExecuteKind,
    ExecuteRequest as EvmExecuteRequest, ExecutionStatus as EvmExecutionStatus,
};
use casper_storage::{
    block_store::types::ApprovalsHashes,
    data_access_layer::{
        balance::BalanceHandling,
        mint::{BalanceIdentifierTransferArgs, BurnRequest},
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
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    TrackingCopy,
};
use casper_types::{
    account::{Account, AccountHash},
    bytesrepr::{self, Bytes, ToBytes, U32_SERIALIZED_LENGTH},
    contracts::NamedKeys,
    evm::{
        Address as EvmAddress, HaltReason as EvmHaltReason, Receipt as EvmReceipt,
        ReceiptStatus as EvmReceiptStatus,
    },
    execution::{Effects, ExecutionResult, TransformKindV2, TransformV2},
    system::handle_payment::ARG_AMOUNT,
    BlockHash, BlockHeader, BlockTime, BlockV2, CLValue, Chainspec, ChecksumRegistry, Digest,
    EntityAddr, EraEndV2, EraId, EvmSpec, FeeHandling, Gas, InvalidTransaction,
    InvalidTransactionV1, Key, ProtocolVersion, PublicKey, RefundHandling, StoredValue, TimeDiff,
    Transaction, TransactionEntryPoint, AUCTION_LANE_ID, MINT_LANE_ID, U512,
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

#[derive(Default)]
struct StaticEvmBlockHashProvider {
    block_hashes: BTreeMap<u64, BlockHash>,
}

impl EvmBlockHashProvider for StaticEvmBlockHashProvider {
    fn block_hash(&self, block_height: u64) -> EvmBlockHashProviderResult<Option<BlockHash>> {
        Ok(self.block_hashes.get(&block_height).copied())
    }
}

fn evm_block_context(
    chainspec: &Chainspec,
    block_height: u64,
    block_time: BlockTime,
    proposer: &PublicKey,
) -> EvmBlockContext {
    EvmBlockContext {
        number: block_height,
        timestamp: block_time.value() / 1000,
        beneficiary: EvmAddress::from_block_proposer_public_key(proposer),
        gas_limit: Some(chainspec.evm_config.block_gas_limit),
        base_fee: Some(chainspec.evm_config.base_fee_wei()),
    }
}

fn write_eip4788_beacon_roots(
    scratch_state: &ScratchGlobalState,
    state_root_hash: Digest,
    chainspec: &Chainspec,
    protocol_version: ProtocolVersion,
    block_context: EvmBlockContext,
    parent_hash: BlockHash,
) -> Result<Digest, BlockExecutionError> {
    if !chainspec.evm_config.enabled || chainspec.evm_config.spec < EvmSpec::Prague {
        return Ok(state_root_hash);
    }

    if block_context.number == 0 {
        return Ok(state_root_hash);
    }

    match scratch_state.block_global(BlockGlobalRequest::set_eip4788_parent_hash(
        state_root_hash,
        protocol_version,
        block_context.timestamp,
        parent_hash,
    )) {
        BlockGlobalResult::RootNotFound => Err(BlockExecutionError::RootNotFound(state_root_hash)),
        BlockGlobalResult::Failure(err) => {
            Err(BlockExecutionError::BlockGlobal(format!("{err:?}")))
        }
        BlockGlobalResult::Success {
            post_state_hash, ..
        } => Ok(post_state_hash),
    }
}

fn evm_precondition_receipt(effective_gas_price: u128) -> EvmReceipt {
    EvmReceipt {
        status: EvmReceiptStatus::Halt(EvmHaltReason::Unknown),
        gas_used: 0,
        effective_gas_price,
        contract_address: None,
        logs: Vec::new(),
    }
}

fn execution_min_cost(
    is_evm: bool,
    gas_limit: Gas,
    cost: U512,
    baseline_motes_amount: U512,
) -> U512 {
    let min_cost = gas_limit.value().min(baseline_motes_amount);
    // EVM cost is already converted to motes. Do not let the raw EVM gas
    // limit raise the minimum above the maximum converted fee.
    if is_evm {
        min_cost.min(cost)
    } else {
        min_cost
    }
}

#[derive(Clone, Debug)]
struct EvmOriginResolution {
    // Concrete payer selected before payment checks. This is deliberately a
    // data-access balance identifier, not an EVM-specific balance mode, so
    // the rest of block execution can use the normal hold/refund/fee
    // machinery.
    balance_identifier: BalanceIdentifier,
    // State mutation to perform later, inside the same tracking copy as EVM
    // execution. Origin resolution itself is read-only so a rejected
    // transaction does not create accounts or links as a side effect.
    identity_plan: EvmIdentityPlan,
}

impl EvmOriginResolution {
    fn new(balance_identifier: BalanceIdentifier, identity_plan: EvmIdentityPlan) -> Self {
        Self {
            balance_identifier,
            identity_plan,
        }
    }
}

/// Deferred write needed to make an EVM sender's identity explicit in global state.
///
/// The runtime makes this decision because it has both pieces of context the
/// executor should not need: the recovered transaction signer and the Casper
/// account view at the current state root.
#[derive(Clone, Copy, Debug)]
enum EvmIdentityPlan {
    /// No identity write is needed. Either the identity already exists, or the
    /// address must remain EVM-native.
    None,
    /// The EVM address has no identity pointer yet, but the recovered signer
    /// already has a Casper account. Link the address to that account hash.
    LinkExisting {
        address: EvmAddress,
        account_hash: AccountHash,
    },
    /// Neither an identity pointer nor a Casper account exists for the
    /// recovered signer. Create the Casper account and then link the EVM
    /// address to it.
    CreateAccount {
        address: EvmAddress,
        account_hash: AccountHash,
        main_purse: casper_types::URef,
    },
}

/// Resolves the payer and any deferred identity write for a signed EVM transaction.
///
/// This function only reads state. That matters because it runs before payment
/// preconditions are known to pass. If execution is later allowed, the returned
/// [`EvmIdentityPlan`] is applied in the tracking copy used for EVM execution.
fn resolve_evm_origin(
    scratch_state: &ScratchGlobalState,
    state_root_hash: Digest,
    protocol_version: ProtocolVersion,
    transaction: &casper_types::EvmTransaction,
) -> Result<EvmOriginResolution, BlockExecutionError> {
    let address = transaction.from();
    // The signer gives us a Casper `AccountHash` preimage from the secp256k1
    // public key. That account hash is not derivable from the 20-byte EVM
    // address alone, so identity linking must happen while the signed
    // transaction is available.
    let signer = transaction
        .signer()
        .map_err(|error| BlockExecutionError::TransactionConversion(error.to_string()))?;
    let account_hash = signer.to_account_hash();
    // Native EVM identities use a deterministic purse derived from the EVM
    // address. Linked Casper accounts use the account's existing main purse
    // instead, so the same key pair can spend the same funds from Casper and
    // Ethereum-style transaction paths.
    let deterministic_purse = casper_types::evm::deterministic_purse(address);
    let mut tracking_copy = scratch_state
        .tracking_copy(state_root_hash)?
        .ok_or(BlockExecutionError::RootNotFound(state_root_hash))?;

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
                    EvmIdentityPlan::None,
                )),
                // Existing EVM-native identities keep paying from their stored
                // purse. We may still plan an upgrade to a Casper link, but
                // only when doing so cannot steal a contract identity or move
                // balances between distinct purses.
                Key::URef(purse) => {
                    let identity_plan = resolve_evm_native_identity_plan(
                        &mut tracking_copy,
                        protocol_version,
                        address,
                        account_hash,
                        purse,
                        deterministic_purse,
                    )?;
                    Ok(EvmOriginResolution::new(
                        BalanceIdentifier::Purse(purse),
                        identity_plan,
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
                    EvmIdentityPlan::None,
                ));
            }
            match account_main_purse(&mut tracking_copy, protocol_version, account_hash)? {
                // A Casper account exists for the recovered signer, but the EVM
                // address has not been seen before. Use the account for payment
                // immediately and write the bridge only if execution proceeds.
                Some(_) => Ok(EvmOriginResolution::new(
                    BalanceIdentifier::Account(account_hash),
                    EvmIdentityPlan::LinkExisting {
                        address,
                        account_hash,
                    },
                )),
                // First use of this signing pair on both sides. Runtime will
                // create a Casper account whose main purse is the deterministic
                // EVM purse, then write the bridge record.
                None => Ok(EvmOriginResolution::new(
                    BalanceIdentifier::Purse(deterministic_purse),
                    EvmIdentityPlan::CreateAccount {
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
) -> Result<EvmIdentityPlan, BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    // Do not overwrite contract identities, and do not turn an arbitrary purse
    // identity into a Casper account link. The only EVM-native identity that is
    // safe to link is the deterministic purse for this address.
    if evm_account_has_code(tracking_copy, address)? || purse.addr() != deterministic_purse.addr() {
        return Ok(EvmIdentityPlan::None);
    }

    match account_main_purse(tracking_copy, protocol_version, account_hash)? {
        // If the recovered Casper account already uses the same deterministic
        // purse, replacing the pointer with `Key::Account` preserves the balance
        // location and lets Casper-native flows see the account identity.
        Some(main_purse) if main_purse.addr() == purse.addr() => {
            Ok(EvmIdentityPlan::LinkExisting {
                address,
                account_hash,
            })
        }
        // A Casper account exists, but its main purse differs from the existing
        // EVM-native purse. Keep the EVM-native identity to avoid moving funds
        // or changing ownership semantics behind the user's back.
        Some(_) => Ok(EvmIdentityPlan::None),
        // No Casper account exists yet, so creating one backed by the existing
        // deterministic purse preserves balances while giving the signer a
        // Casper account identity.
        None => Ok(EvmIdentityPlan::CreateAccount {
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

fn evm_account_has_nonce<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: EvmAddress,
) -> Result<bool, BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    let key = Key::Evm(casper_types::EvmAddr::Nonce(address));
    match tracking_copy
        .read(&key)
        .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?
    {
        Some(StoredValue::CLValue(cl_value)) => {
            let _nonce = cl_value
                .into_t::<u64>()
                .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
            Ok(true)
        }
        Some(stored_value) => Err(BlockExecutionError::PaymentError(format!(
            "unexpected stored value for {key}: expected StoredValue::CLValue(u64), found {}",
            stored_value.type_name()
        ))),
        None => Ok(false),
    }
}

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

fn apply_evm_proposer_identity<R>(
    tracking_copy: &mut TrackingCopy<R>,
    protocol_version: ProtocolVersion,
    proposer: &PublicKey,
) -> Result<(), BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    let address = EvmAddress::from_block_proposer_public_key(proposer);
    let account_hash = proposer.to_account_hash();
    if account_main_purse(tracking_copy, protocol_version, account_hash)?.is_none() {
        return Ok(());
    }

    let identity_key = Key::Evm(casper_types::EvmAddr::Account(address));
    match tracking_copy
        .read(&identity_key)
        .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?
    {
        Some(StoredValue::CLValue(cl_value)) => {
            let identity = cl_value
                .into_t::<Key>()
                .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
            match identity {
                Key::Account(_) | Key::URef(_) => Ok(()),
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
            if evm_account_has_code(tracking_copy, address)?
                || evm_account_has_nonce(tracking_copy, address)?
            {
                return Ok(());
            }
            write_evm_identity(tracking_copy, address, Key::Account(account_hash))
        }
    }
}

fn apply_evm_identity_plan<R>(
    tracking_copy: &mut TrackingCopy<R>,
    protocol_version: ProtocolVersion,
    plan: EvmIdentityPlan,
) -> Result<(), BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    // Identity writes are intentionally delayed until after payment
    // preconditions pass. They are applied to the same tracking copy as EVM
    // execution so the identity record and nonce/code/storage updates commit or
    // discard together.
    match plan {
        EvmIdentityPlan::None => Ok(()),
        EvmIdentityPlan::LinkExisting {
            address,
            account_hash,
        } => write_evm_identity(tracking_copy, address, Key::Account(account_hash)),
        EvmIdentityPlan::CreateAccount {
            address,
            account_hash,
            main_purse,
        } => {
            // Another transaction in the same block may have already created
            // the account through this scratch state. Avoid recreating it, but
            // still write the EVM identity pointer below.
            if account_main_purse(tracking_copy, protocol_version, account_hash)?.is_none() {
                let account = Account::create(account_hash, NamedKeys::new(), main_purse);
                tracking_copy
                    .create_addressable_entity_from_account(account, protocol_version)
                    .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
            }
            write_evm_identity(tracking_copy, address, Key::Account(account_hash))
        }
    }
}

fn write_evm_identity<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: EvmAddress,
    identity: Key,
) -> Result<(), BlockExecutionError>
where
    R: StateReader<Key, StoredValue, Error = casper_storage::global_state::error::Error>,
{
    // Keep the bridge record minimal: a CLValue containing the identity `Key`.
    // Nonce, code hash, bytecode, and storage live under their own EVM keys.
    let key = Key::Evm(casper_types::EvmAddr::Account(address));
    let cl_value = CLValue::from_t(identity)
        .map_err(|error| BlockExecutionError::PaymentError(error.to_string()))?;
    tracking_copy.write(key, StoredValue::CLValue(cl_value));
    Ok(())
}

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
    execution_engine_v1: &ExecutionEngineV1,
    execution_engine_v2: ExecutorV2,
    chainspec: &Chainspec,
    metrics: Option<Arc<Metrics>>,
    execution_pre_state: ExecutionPreState,
    evm_block_hash_provider: &dyn EvmBlockHashProvider,
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

    state_root_hash = write_eip4788_beacon_roots(
        &scratch_state,
        state_root_hash,
        chainspec,
        protocol_version,
        evm_block_context(chainspec, block_height, block_time, &proposer),
        parent_hash,
    )?;

    let transaction_config = &chainspec.transaction_config;

    for stored_transaction in executable_block.transactions {
        let evm_transaction = stored_transaction.as_evm();
        let is_evm = evm_transaction.is_some();
        let transaction = MetaTransaction::from_transaction(
            &stored_transaction,
            chainspec.core_config.pricing_handling,
            transaction_config,
        )
        .map_err(|err| BlockExecutionError::TransactionConversion(err.to_string()))?;

        let transaction_hash = stored_transaction.hash();
        let authorization_keys = stored_transaction.authorization_keys();

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

        let lane_id = transaction.transaction_lane();

        let mut artifact_builder = {
            // NOTE: this is the allowed computation limit (gas limit)
            let gas_limit = if let Some(evm_transaction) = evm_transaction {
                Gas::new(evm_transaction.gas_limit())
            } else {
                match transaction.gas_limit(chainspec) {
                    Ok(gas) => gas,
                    Err(ite) => {
                        debug!(%transaction_hash, %ite, "invalid transaction (gas limit)");
                        artifacts.push(
                            ExecutionArtifactBuilder::pre_condition_failure(
                                &stored_transaction,
                                current_gas_price,
                                ite,
                            )
                            .build(),
                        );
                        continue;
                    }
                }
            };

            // NOTE: this is the actual adjusted cost that we charge for.
            // Native transactions use gas limit * Casper gas price. EVM
            // transactions convert gas limit * EVM gas price from wei to motes.
            // For accepted EIP-1559 transactions, config compliance has already required
            // `max_priority_fee_per_gas == 0`, so the effective EVM gas price is the
            // configured base fee capped by `max_fee_per_gas`; Casper does not charge an
            // Ethereum-style priority premium while transaction priority is not based on
            // gas parameters.
            let cost = if let Some(evm_transaction) = evm_transaction {
                evm_transaction
                    .max_fee_amount(&chainspec.evm_config)
                    .ok_or_else(|| {
                        BlockExecutionError::PaymentError(
                            "EVM fee amount overflowed U512".to_string(),
                        )
                    })?
            } else {
                match stored_transaction.gas_cost(chainspec, lane_id, current_gas_price) {
                    Ok(motes) => motes.value(),
                    Err(ite) => {
                        debug!(%transaction_hash, "invalid transaction (motes conversion)");
                        artifacts.push(
                            ExecutionArtifactBuilder::pre_condition_failure(
                                &stored_transaction,
                                current_gas_price,
                                ite,
                            )
                            .build(),
                        );
                        continue;
                    }
                }
            };

            // this is the minimum we will charge, even if 0 is consumed
            let min_cost = execution_min_cost(is_evm, gas_limit, cost, baseline_motes_amount);
            ExecutionArtifactBuilder::new(
                &stored_transaction,
                gas_limit,
                current_gas_price,
                cost,
                min_cost,
            )
        };

        let is_standard_payment = transaction.is_standard_payment();
        let is_custom_payment = !is_standard_payment && transaction.is_custom_payment();
        let is_v1_wasm = transaction.is_v1_wasm();
        let is_v2_wasm = transaction.is_v2_wasm();
        let initiator_addr = stored_transaction.initiator_addr();
        let evm_origin_resolution = if let Some(evm_transaction) = evm_transaction {
            Some(resolve_evm_origin(
                &scratch_state,
                state_root_hash,
                protocol_version,
                evm_transaction,
            )?)
        } else {
            None
        };
        let payer_balance_identifier = if let Some(resolution) = &evm_origin_resolution {
            resolution.balance_identifier.clone()
        } else {
            initiator_addr
                .clone()
                .try_into()
                .map_err(|_| BlockExecutionError::InvalidTransactionVariant)?
        };

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
                    target: Box::new(payer_balance_identifier.clone()),
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
                payer_balance_identifier.clone(),
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
                if let Some(evm_transaction) = evm_transaction {
                    artifact_builder.with_zero_cost().with_evm_receipt(
                        evm_precondition_receipt(
                            evm_transaction
                                .effective_gas_price(chainspec.evm_config.base_fee_wei()),
                        ),
                        U512::zero(),
                        Effects::new(),
                    );
                }
                artifacts.push(artifact_builder.build());
                // only reads have happened so far, and we can't charge due
                // to insufficient balance, so move on with no effects committed
                continue;
            }
        }

        let mut balance_identifier = {
            if is_evm {
                // EVM transactions intentionally do not participate in Casper custom payment
                // or refund-purse setup. Ethereum payloads carry a gas limit and gas price fields,
                // but this chain still owns the fee/refund policy through the same chainspec
                // settings used by Deploy and native Transaction::V1 payloads. The EVM sender's
                // main purse is therefore the payer for the processing hold, refund
                // calculation, and final fee handling, while revm runs with gas fee
                // charging disabled and only mutates EVM nonce, code, storage,
                // logs, creates, and value transfers.
                payer_balance_identifier.clone()
            } else if is_standard_payment {
                let contract_might_pay =
                    addressable_entity_enabled && transaction.is_contract_by_hash_invocation();

                if contract_might_pay {
                    match invoked_contract_will_pay(&scratch_state, state_root_hash, &transaction) {
                        Ok(Some(entity_addr)) => BalanceIdentifier::Entity(entity_addr),
                        Ok(None) => {
                            // the initiating account pays using its main purse
                            trace!(%transaction_hash, "direct invocation with account payment");
                            payer_balance_identifier.clone()
                        }
                        Err(err) => {
                            trace!(%transaction_hash, "failed to resolve contract self payment");
                            artifact_builder
                                .with_state_result_error(err)
                                .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                            BalanceIdentifier::PenalizedAccount(
                                initiator_addr
                                    .account_hash()
                                    .ok_or(BlockExecutionError::InvalidTransactionVariant)?,
                            )
                        }
                    }
                } else {
                    // the initiating account pays using its main purse
                    trace!(%transaction_hash, "account session with standard payment");
                    payer_balance_identifier.clone()
                }
            } else if is_v2_wasm {
                // vm2 does not support custom payment, so it MUST be standard payment
                // if transaction runtime is v2 then the initiating account will pay using
                // the refund purse
                payer_balance_identifier.clone()
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
                            .insert(ARG_AMOUNT, artifact_builder.cost_to_use())
                            .map_err(|e| BlockExecutionError::PaymentError(e.to_string()))?;
                        execution_engine_v1.execute(&scratch_state, pay_request)
                    }
                    Err(error) => {
                        WasmV1Result::invalid_executable_item(custom_payment_gas_limit, error)
                    }
                };

                let insufficient_payment_deposited = !pay_result.balance_increased_by_amount(
                    payment_balance_addr,
                    artifact_builder.cost_to_use(),
                );

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
                            payer_balance_identifier.clone(),
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
                BalanceIdentifier::PenalizedAccount(
                    initiator_addr
                        .account_hash()
                        .ok_or(BlockExecutionError::InvalidTransactionVariant)?,
                )
            }
        };

        let post_payment_balance_result = scratch_state.balance(BalanceRequest::new(
            state_root_hash,
            protocol_version,
            balance_identifier.clone(),
            balance_handling,
            ProofHandling::NoProofs,
        ));

        artifact_builder.with_available(post_payment_balance_result.available_balance().copied());
        let allow_execution = {
            let is_not_penalized = !balance_identifier.is_penalty();
            // in the case of custom payment, we do all payment processing up front after checking
            // if the initiator can cover the penalty payment, and then either charge the full
            // amount in the happy path or the penalty amount in the sad path...in whichever case
            // the sad path is handled by is_penalty and the balance in the payment purse is
            // the penalty payment or the full amount but is 'sufficient' either way
            let actual_cost = artifact_builder.actual_cost(); // use actual cost here
            let required_balance = if let Some(evm_transaction) = evm_transaction {
                evm_transaction
                    .required_balance(actual_cost)
                    .ok_or_else(|| {
                        BlockExecutionError::PaymentError(
                            "EVM value plus fee amount overflowed U512".to_string(),
                        )
                    })?
            } else {
                actual_cost
            };
            let is_sufficient_balance =
                is_custom_payment || post_payment_balance_result.is_sufficient(required_balance);
            let is_allowed_by_chainspec = chainspec.is_supported(lane_id);
            let allow = is_not_penalized && is_sufficient_balance && is_allowed_by_chainspec;
            if !allow {
                let err_msg = {
                    if !is_sufficient_balance {
                        "Insufficient funds".to_string()
                    } else {
                        format!(
                            "penalized: {}, sufficient balance: {}, allowed by chainspec: {}",
                            !is_not_penalized, is_sufficient_balance, is_allowed_by_chainspec
                        )
                    }
                };
                if artifact_builder.error_message().is_none() {
                    artifact_builder.with_error_message(err_msg);
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
                let hold_amount = artifact_builder.cost_to_use();
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
                    let transaction_args = transaction.session_args();
                    let runtime_args = transaction_args
                        .as_named()
                        .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
                    let entry_point = transaction.entry_point();
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
                            .commit_effects(state_root_hash, transfer_result.effects().clone())?;
                        artifact_builder
                            .consume_limit()
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
                            .commit_effects(state_root_hash, burn_result.effects().clone())?;
                        artifact_builder
                            .consume_limit()
                            .with_burn_result(burn_result)
                            .map_err(|_| BlockExecutionError::RootNotFound(state_root_hash))?;
                    } else {
                        artifact_builder.with_error_message(format!(
                            "Attempt to call unsupported native mint entrypoint: {}",
                            entry_point
                        ));
                    }
                }
                lane_id if lane_id == AUCTION_LANE_ID => {
                    let transaction_args = transaction.session_args();
                    let runtime_args = transaction_args
                        .as_named()
                        .ok_or(BlockExecutionError::InvalidTransactionArgs)?;
                    let entry_point = transaction.entry_point();
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
                            state_root_hash = scratch_state.commit_effects(
                                state_root_hash,
                                bidding_result.effects().clone(),
                            )?;
                            artifact_builder
                                .consume_limit()
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
                _ if is_evm => {
                    let evm_transaction = evm_transaction.expect("EVM transaction should exist");
                    let base_fee_wei = chainspec.evm_config.base_fee_wei();
                    let block_context =
                        evm_block_context(chainspec, block_height, block_time, &proposer);
                    let request = EvmExecuteRequest {
                        block: block_context,
                        kind: EvmExecuteKind::Transaction(Box::new(evm_transaction.clone())),
                    };
                    let mut tracking_copy = scratch_state
                        .tracking_copy(state_root_hash)?
                        .ok_or(BlockExecutionError::RootNotFound(state_root_hash))?;
                    if let Some(resolution) = &evm_origin_resolution {
                        // Apply the deferred bridge/account creation only now,
                        // after balance preconditions have allowed execution.
                        // This keeps rejected EVM transactions from mutating
                        // identity state and makes the identity write atomic
                        // with the revm state transition below.
                        apply_evm_identity_plan(
                            &mut tracking_copy,
                            protocol_version,
                            resolution.identity_plan,
                        )?;
                    }
                    apply_evm_proposer_identity(&mut tracking_copy, protocol_version, &proposer)?;
                    let outcome = EvmExecutor::new(chainspec.evm_config)
                        .execute_with_block_hash_provider(
                            &mut tracking_copy,
                            request,
                            evm_block_hash_provider,
                        )
                        .map_err(|error| {
                            BlockExecutionError::TransactionConversion(error.to_string())
                        })?;
                    let execution_effects = tracking_copy.effects();
                    state_root_hash =
                        scratch_state.commit_effects(state_root_hash, execution_effects.clone())?;
                    let effective_gas_price = evm_transaction.effective_gas_price(base_fee_wei);
                    let consumed = if matches!(outcome.status, EvmExecutionStatus::Success) {
                        evm_transaction
                            .fee_amount(outcome.gas_used, &chainspec.evm_config)
                            .ok_or_else(|| {
                                BlockExecutionError::PaymentError(
                                    "EVM fee amount overflowed U512".to_string(),
                                )
                            })?
                    } else {
                        artifact_builder.cost_to_use()
                    };
                    artifact_builder.with_evm_receipt(
                        outcome.to_receipt(effective_gas_price),
                        consumed,
                        execution_effects,
                    );
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
                        artifact_builder.gas_limit(),
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
                    artifact_builder.gas_limit(),
                    chainspec.network_config.name.clone(),
                    state_root_hash,
                    parent_block_hash,
                    block_height,
                    &transaction,
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

        if is_evm && !allow_execution {
            let effective_gas_price = evm_transaction
                .expect("EVM transaction should exist")
                .effective_gas_price(chainspec.evm_config.base_fee_wei());
            artifact_builder.with_zero_cost().with_evm_receipt(
                evm_precondition_receipt(effective_gas_price),
                U512::zero(),
                Effects::new(),
            );
            artifacts.push(artifact_builder.build());
            continue;
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
            let consumed =
                if balance_identifier.is_penalty() || artifact_builder.error_message().is_some() {
                    artifact_builder.cost_to_use() // no refund for penalty
                } else {
                    artifact_builder.consumed()
                };

            let available = artifact_builder.available().unwrap_or(U512::zero());

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
                            limit: artifact_builder.limit(),
                            gas_price: current_gas_price,
                            cost: artifact_builder.cost_to_use(),
                        })
                    } else {
                        None
                    }
                }
                RefundHandling::Burn { refund_ratio } => {
                    let (limit, gas_price) = if is_evm {
                        (artifact_builder.cost_to_use(), 1)
                    } else {
                        (artifact_builder.limit(), current_gas_price)
                    };
                    Some(HandleRefundMode::Burn {
                        limit,
                        gas_price,
                        cost: artifact_builder.cost_to_use(),
                        consumed,
                        source: Box::new(balance_identifier.clone()),
                        ratio: refund_ratio,
                        available,
                    })
                }
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
                        Some(HandleRefundMode::Refund {
                            initiator_addr: Box::new(initiator_addr.clone()),
                            limit: artifact_builder.limit(),
                            gas_price: current_gas_price,
                            consumed,
                            cost: artifact_builder.cost_to_use(),
                            ratio: refund_ratio,
                            source,
                            target: Box::new(BalanceIdentifier::Refund),
                            available,
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
                        let (limit, gas_price) = if is_evm {
                            (artifact_builder.cost_to_use(), 1)
                        } else {
                            (artifact_builder.limit(), current_gas_price)
                        };
                        Some(HandleRefundMode::CalculateAmount {
                            limit,
                            gas_price,
                            consumed,
                            cost: artifact_builder.cost_to_use(),
                            ratio: refund_ratio,
                            available,
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
        artifact_builder.with_refund_amount(refund_amount);

        // take the lower of the difference between cost - refund OR available
        let fee_amount = artifact_builder
            .cost_to_use()
            .saturating_sub(refund_amount)
            .min(artifact_builder.available().unwrap_or(U512::zero()));

        // handle fees per the chainspec determined setting.
        let handle_fee_result = match fee_handling {
            FeeHandling::NoFee => {
                // in this mode, a gas hold is placed on the payer's purse.
                let hold_request = BalanceHoldRequest::new_gas_hold(
                    state_root_hash,
                    protocol_version,
                    balance_identifier,
                    fee_amount,
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
    block_hashes: BTreeMap<u64, BlockHash>,
    input_transaction: Transaction,
) -> SpeculativeExecutionResult
where
    S: StateProvider,
{
    let transaction_config = &chainspec.transaction_config;
    let maybe_transaction = MetaTransaction::from_transaction(
        &input_transaction,
        chainspec.core_config.pricing_handling,
        transaction_config,
    );
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
            casper_types::EvmTransactionError::Disabled,
        ));
    }
    if evm_transaction.gas_limit() > chainspec.evm_config.block_gas_limit {
        return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
            casper_types::EvmTransactionError::GasLimitExceedsBlockGasLimit {
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
                casper_types::EvmTransactionError::Decode(format!(
                    "state root {state_root_hash} not found"
                )),
            ))
        }
        Err(error) => {
            return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
                casper_types::EvmTransactionError::Decode(format!(
                    "failed to check out EVM speculative execution state: {error}"
                )),
            ))
        }
    };
    let block_time = block_header
        .timestamp()
        .saturating_add(chainspec.core_config.minimum_block_time);
    let base_fee_wei = chainspec.evm_config.base_fee_wei();
    let block_context = EvmBlockContext {
        number: block_header.height(),
        timestamp: block_time.millis() / 1000,
        beneficiary: EvmAddress::ZERO,
        gas_limit: Some(chainspec.evm_config.block_gas_limit),
        base_fee: Some(base_fee_wei),
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
    let block_hash_provider = StaticEvmBlockHashProvider { block_hashes };
    let outcome = match EvmExecutor::new(chainspec.evm_config).execute_with_block_hash_provider(
        &mut tracking_copy,
        execute_request,
        &block_hash_provider,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return SpeculativeExecutionResult::invalid_transaction(InvalidTransaction::Evm(
                casper_types::EvmTransactionError::Decode(error.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use casper_storage::{global_state::state, tracking_copy::TrackingCopyExt};
    use casper_types::{EvmConfig, DEFAULT_WEI_PER_MOTE};

    #[test]
    fn should_not_raise_evm_min_cost_above_converted_fee() {
        let gas_limit = Gas::new(21_000);
        let cost = U512::from(1);
        let baseline_motes_amount = U512::from(1_000_000);

        assert_eq!(
            execution_min_cost(true, gas_limit, cost, baseline_motes_amount),
            cost
        );
    }

    #[test]
    fn should_keep_native_min_cost_based_on_gas_limit() {
        let gas_limit = Gas::new(21_000);
        let cost = U512::from(1);
        let baseline_motes_amount = U512::from(1_000_000);

        assert_eq!(
            execution_min_cost(false, gas_limit, cost, baseline_motes_amount),
            U512::from(21_000)
        );
    }

    #[test]
    fn eip4788_hook_writes_beacon_roots_without_transactions() {
        let chainspec = Chainspec {
            evm_config: EvmConfig {
                enabled: true,
                chain_id: 7,
                spec: EvmSpec::Prague,
                block_gas_limit: 30_000_000,
                base_fee: 0,
                wei_per_mote: DEFAULT_WEI_PER_MOTE,
            },
            ..Default::default()
        };
        let (global_state, state_root_hash, _tempdir) =
            state::lmdb::make_temporary_global_state([]);
        let scratch_state = global_state.create_scratch();
        let block_time = BlockTime::new(2_000);
        let block_context = evm_block_context(&chainspec, 1, block_time, &PublicKey::System);
        let parent_hash = BlockHash::new(Digest::from_raw([0x44; 32]));

        let updated_state_root_hash = write_eip4788_beacon_roots(
            &scratch_state,
            state_root_hash,
            &chainspec,
            ProtocolVersion::V1_0_0,
            block_context.clone(),
            parent_hash,
        )
        .expect("EIP-4788 hook should succeed");
        let tracking_copy = scratch_state
            .tracking_copy(updated_state_root_hash)
            .expect("tracking copy should not fail")
            .expect("state root should exist");
        let entry = tracking_copy
            .get_eip4788_parent_hash(block_context.timestamp)
            .expect("EIP-4788 beacon root should be readable");

        assert_eq!(entry, Some((block_context.timestamp, parent_hash)));
    }
}
