//! RPC related to speculative execution.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{str, sync::Arc};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_execution_engine::engine_state::Error as EngineStateError;
use casper_json_rpc::ReservedErrorCode;
use casper_types::{BlockHash, Deploy, ExecutionResult, JsonBlock, ProtocolVersion};

use super::{
    chain::BlockIdentifier,
    common,
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, RpcWithParams,
};
use crate::{components::contract_runtime::SpeculativeExecutionState, effect::EffectBuilder};

static SPECULATIVE_EXEC_PARAMS: Lazy<SpeculativeExecParams> = Lazy::new(|| SpeculativeExecParams {
    block_identifier: Some(BlockIdentifier::Hash(JsonBlock::doc_example().hash)),
    deploy: Deploy::doc_example().clone(),
});
static SPECULATIVE_EXEC_RESULT: Lazy<SpeculativeExecResult> = Lazy::new(|| SpeculativeExecResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    block_hash: JsonBlock::doc_example().hash,
    execution_result: ExecutionResult::example().clone(),
});

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

/// Result for "speculative_exec" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeculativeExecResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// Hash of the block on top of which the deploy was executed.
    pub block_hash: BlockHash,
    /// Result of the execution.
    pub execution_result: ExecutionResult,
}

impl DocExample for SpeculativeExecResult {
    fn doc_example() -> &'static Self {
        &SPECULATIVE_EXEC_RESULT
    }
}

/// "speculative_exec" RPC
pub struct SpeculativeExec {}

#[async_trait]
impl RpcWithParams for SpeculativeExec {
    const METHOD: &'static str = "speculative_exec";
    type RequestParams = SpeculativeExecParams;
    type ResponseResult = SpeculativeExecResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let SpeculativeExecParams {
            block_identifier: maybe_block_id,
            deploy,
        } = params;
        let deploy = Arc::new(deploy);
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

        let accept_deploy_result = effect_builder
            .try_accept_deploy(Arc::clone(&deploy), Some(Box::new(block.take_header())))
            .await;

        if let Err(error) = accept_deploy_result {
            return Err(Error::new(ErrorCode::InvalidDeploy, error.to_string()));
        }

        let result = effect_builder
            .speculative_execute_deploy(execution_prestate, Arc::clone(&deploy))
            .await;

        match result {
            Ok(Some(execution_result)) => {
                let result = Self::ResponseResult {
                    api_version,
                    block_hash,
                    execution_result,
                };
                Ok(result)
            }
            Ok(None) => Err(Error::new(
                ErrorCode::NoSuchBlock,
                "block hash not found".to_string(),
            )),
            Err(error) => {
                let rpc_error = match error {
                    EngineStateError::RootNotFound(_) => Error::new(ErrorCode::NoSuchStateRoot, ""),
                    EngineStateError::WasmPreprocessing(error) => {
                        Error::new(ErrorCode::InvalidDeploy, error.to_string())
                    }
                    EngineStateError::InvalidDeployItemVariant(error) => {
                        Error::new(ErrorCode::InvalidDeploy, error)
                    }
                    EngineStateError::InvalidProtocolVersion(_) => Error::new(
                        ErrorCode::InvalidDeploy,
                        format!("deploy used invalid protocol version {}", error),
                    ),
                    EngineStateError::Deploy => Error::new(ErrorCode::InvalidDeploy, ""),
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
                    | EngineStateError::MissingSystemContractRegistry
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
}
