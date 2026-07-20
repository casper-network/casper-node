use crate::types::{DataSize, TransactionHeader};
use casper_types::{contract_messages::Messages, execution::ExecutionResult, TransactionHash};
use serde::Serialize;

#[derive(Clone, Debug, DataSize, PartialEq, Eq, Serialize)]
pub(crate) struct ExecutionArtifact {
    pub(crate) transaction_hash: TransactionHash,
    pub(crate) transaction_header: TransactionHeader,
    pub(crate) execution_result: ExecutionResult,
    pub(crate) messages: Messages,
}

impl ExecutionArtifact {
    pub(crate) fn new(
        transaction_hash: TransactionHash,
        transaction_header: TransactionHeader,
        execution_result: ExecutionResult,
        messages: Messages,
    ) -> Self {
        Self {
            transaction_hash,
            transaction_header,
            execution_result,
            messages,
        }
    }
}
