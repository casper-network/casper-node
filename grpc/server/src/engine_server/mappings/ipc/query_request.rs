use std::convert::{TryFrom, TryInto};

use node::components::contract_runtime::core::engine_state::query::QueryRequest;
use node::components::contract_runtime::shared::newtypes::BLAKE2B_DIGEST_LENGTH;

use crate::engine_server::{ipc, mappings::MappingError};

impl TryFrom<ipc::QueryRequest> for QueryRequest {
    type Error = MappingError;

    fn try_from(mut query_request: ipc::QueryRequest) -> Result<Self, Self::Error> {
        let state_hash = {
            let state_hash = query_request.get_state_hash();
            let length = state_hash.len();
            if length != BLAKE2B_DIGEST_LENGTH {
                return Err(MappingError::InvalidStateHashLength {
                    expected: BLAKE2B_DIGEST_LENGTH,
                    actual: length,
                });
            }
            state_hash
                .try_into()
                .map_err(|_| MappingError::TryFromSlice)?
        };

        let key = query_request
            .take_base_key()
            .try_into()
            .map_err(MappingError::Parsing)?;

        let path = query_request.take_path().into_vec();

        Ok(QueryRequest::new(state_hash, key, path))
    }
}
