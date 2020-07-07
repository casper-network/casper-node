use std::convert::{TryFrom, TryInto};

use node::contract_core::engine_state::{
    execute_request::ExecuteRequest, execution_result::ExecutionResult,
};
use node::contract_shared::newtypes::BLAKE2B_DIGEST_LENGTH;

use crate::engine_server::{ipc, mappings::MappingError};

impl TryFrom<ipc::ExecuteRequest> for ExecuteRequest {
    type Error = ipc::ExecuteResponse;

    fn try_from(mut request: ipc::ExecuteRequest) -> Result<Self, Self::Error> {
        let parent_state_hash = {
            let parent_state_hash = request.take_parent_state_hash();
            let length = parent_state_hash.len();
            if length != BLAKE2B_DIGEST_LENGTH {
                let mut result = ipc::ExecuteResponse::new();
                result.mut_missing_parent().set_hash(parent_state_hash);
                return Err(result);
            }
            parent_state_hash.as_slice().try_into().map_err(|_| {
                let mut result = ipc::ExecuteResponse::new();
                result
                    .mut_missing_parent()
                    .set_hash(parent_state_hash.clone());
                result
            })?
        };

        let block_time = request.get_block_time();

        let deploys = Into::<Vec<_>>::into(request.take_deploys())
            .into_iter()
            .map(|deploy_item| {
                deploy_item
                    .try_into()
                    .map_err(|err: MappingError| ExecutionResult::precondition_failure(err.into()))
            })
            .collect();

        let protocol_version = request.take_protocol_version().into();

        Ok(ExecuteRequest::new(
            parent_state_hash,
            block_time,
            deploys,
            protocol_version,
        ))
    }
}

impl From<ExecuteRequest> for ipc::ExecuteRequest {
    fn from(req: ExecuteRequest) -> Self {
        let mut result = ipc::ExecuteRequest::new();
        result.set_parent_state_hash(req.parent_state_hash.to_vec());
        result.set_block_time(req.block_time);
        result.set_deploys(
            req.deploys
                .into_iter()
                .map(|res| match res {
                    Ok(deploy_item) => deploy_item.into(),
                    Err(_) => ipc::DeployItem::new(),
                })
                .collect(),
        );
        result.set_protocol_version(req.protocol_version.into());
        result
    }
}
