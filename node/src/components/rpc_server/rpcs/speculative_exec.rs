//! RPC related to speculative execution.

use std::str;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_execution_engine::engine_state::Error as EngineStateError;
use casper_json_rpc::ReservedErrorCode;
use casper_types::{
    contract_messages::Messages, execution::ExecutionResultV2, BlockHash, Deploy, ProtocolVersion,
    Transaction,
};

use super::{
    chain::BlockIdentifier,
    common,
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, RpcWithParams,
};
use crate::{
    components::contract_runtime::{SpeculativeExecutionError, SpeculativeExecutionState},
    effect::EffectBuilder,
};

static SPECULATIVE_EXEC_TXN_PARAMS: Lazy<SpeculativeExecTxnParams> =
    Lazy::new(|| SpeculativeExecTxnParams {
        block_identifier: Some(BlockIdentifier::Hash(*BlockHash::example())),
        transaction: Transaction::doc_example().clone(),
    });
static SPECULATIVE_EXEC_TXN_RESULT: Lazy<SpeculativeExecTxnResult> =
    Lazy::new(|| SpeculativeExecTxnResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        block_hash: *BlockHash::example(),
        execution_result: ExecutionResultV2::example().clone(),
        messages: Vec::new(),
    });
static SPECULATIVE_EXEC_PARAMS: Lazy<SpeculativeExecParams> = Lazy::new(|| SpeculativeExecParams {
    block_identifier: Some(BlockIdentifier::Hash(*BlockHash::example())),
    deploy: Deploy::doc_example().clone(),
});

/// Params for "speculative_exec_txn" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeculativeExecTxnParams {
    /// Block hash on top of which to execute the transaction.
    pub block_identifier: Option<BlockIdentifier>,
    /// Transaction to execute.
    pub transaction: Transaction,
}

impl DocExample for SpeculativeExecTxnParams {
    fn doc_example() -> &'static Self {
        &SPECULATIVE_EXEC_TXN_PARAMS
    }
}

/// Result for "speculative_exec_txn" and "speculative_exec" RPC responses.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeculativeExecTxnResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// Hash of the block on top of which the transaction was executed.
    pub block_hash: BlockHash,
    /// Result of the execution.
    pub execution_result: ExecutionResultV2,
    /// Messages emitted during execution.
    pub messages: Messages,
}

impl DocExample for SpeculativeExecTxnResult {
    fn doc_example() -> &'static Self {
        &SPECULATIVE_EXEC_TXN_RESULT
    }
}

/// "speculative_exec_txn" RPC
pub struct SpeculativeExecTxn {}

#[async_trait]
impl RpcWithParams for SpeculativeExecTxn {
    const METHOD: &'static str = "speculative_exec_txn";
    type RequestParams = SpeculativeExecTxnParams;
    type ResponseResult = SpeculativeExecTxnResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        handle_request(
            effect_builder,
            api_version,
            params.block_identifier,
            params.transaction,
        )
        .await
    }
}

/// Params for "speculative_exec" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeculativeExecParams {
    /// Block hash on top of which to execute the deploy.
    pub block_identifier: Option<BlockIdentifier>,
    /// Deploy to execute.
    pub deploy: Deploy,
}

impl DocExample for SpeculativeExecParams {
    fn doc_example() -> &'static Self {
        &SPECULATIVE_EXEC_PARAMS
    }
}

/// "speculative_exec" RPC
pub struct SpeculativeExec {}

#[async_trait]
impl RpcWithParams for SpeculativeExec {
    const METHOD: &'static str = "speculative_exec";
    type RequestParams = SpeculativeExecParams;
    type ResponseResult = SpeculativeExecTxnResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        handle_request(
            effect_builder,
            api_version,
            params.block_identifier,
            Transaction::from(params.deploy),
        )
        .await
    }
}

async fn handle_request<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
    maybe_block_id: Option<BlockIdentifier>,
    transaction: Transaction,
) -> Result<SpeculativeExecTxnResult, Error> {
    let only_from_available_block_range = true;

    let block = common::get_block(
        maybe_block_id,
        only_from_available_block_range,
        effect_builder,
    )
    .await?;
    let block_hash = *block.hash();
    let execution_prestate = SpeculativeExecutionState {
        state_root_hash: *block.state_root_hash(),
        block_time: block.timestamp(),
        protocol_version: block.protocol_version(),
    };

    let accept_transaction_result = effect_builder
        .try_accept_transaction(transaction.clone(), Some(Box::new(block.take_header())))
        .await;

    if let Err(error) = accept_transaction_result {
        return Err(Error::new(ErrorCode::InvalidTransaction, error.to_string()));
    }

    let result = effect_builder
        .speculatively_execute(execution_prestate, Box::new(transaction))
        .await;

    match result {
        Ok((execution_result, messages)) => {
            let result = SpeculativeExecTxnResult {
                api_version,
                block_hash,
                execution_result,
                messages,
            };
            Ok(result)
        }
        Err(SpeculativeExecutionError::NewRequest(error)) => {
            let rpc_error = Error::new(ErrorCode::InvalidTransaction, error.to_string());
            Err(rpc_error)
        }
        Err(SpeculativeExecutionError::EngineState(error)) => {
            let rpc_error = match error {
                EngineStateError::RootNotFound(_) => Error::new(ErrorCode::NoSuchStateRoot, ""),
                EngineStateError::WasmPreprocessing(error) => {
                    Error::new(ErrorCode::InvalidTransaction, error.to_string())
                }
                EngineStateError::InvalidDeployItemVariant(error) => {
                    Error::new(ErrorCode::InvalidTransaction, error)
                }
                EngineStateError::InvalidProtocolVersion(_) => Error::new(
                    ErrorCode::InvalidTransaction,
                    format!("deploy used invalid protocol version {}", error),
                ),
                EngineStateError::Deploy => Error::new(ErrorCode::InvalidTransaction, ""),
                EngineStateError::Genesis(_)
                | EngineStateError::WasmSerialization(_)
                | EngineStateError::Exec(_)
                | EngineStateError::Storage(_)
                | EngineStateError::Authorization
                | EngineStateError::InsufficientPayment
                | EngineStateError::GasConversionOverflow
                | EngineStateError::Finalization
                | EngineStateError::Bytesrepr(_)
                | EngineStateError::Mint(_)
                | EngineStateError::InvalidKeyVariant
                | EngineStateError::ProtocolUpgrade(_)
                | EngineStateError::CommitError(_)
                | EngineStateError::MissingSystemEntityRegistry
                | EngineStateError::MissingSystemContractHash(_)
                | EngineStateError::RuntimeStackOverflow
                | EngineStateError::FailedToGetKeys(_)
                | EngineStateError::FailedToGetStoredWithdraws
                | EngineStateError::FailedToGetWithdrawPurses
                | EngineStateError::FailedToRetrieveUnbondingDelay
                | EngineStateError::FailedToRetrieveEraId => {
                    Error::new(ReservedErrorCode::InternalError, error.to_string())
                }
                _ => Error::new(
                    ReservedErrorCode::InternalError,
                    format!("Unhandled engine state error: {}", error),
                ),
            };
            Err(rpc_error)
        }
    }
}
