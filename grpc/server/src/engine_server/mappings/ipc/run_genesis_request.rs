use std::convert::{TryFrom, TryInto};

use node::contract_core::engine_state::run_genesis_request::RunGenesisRequest;

use crate::engine_server::{ipc, mappings::MappingError};

impl TryFrom<ipc::RunGenesisRequest> for RunGenesisRequest {
    type Error = MappingError;

    fn try_from(mut run_genesis_request: ipc::RunGenesisRequest) -> Result<Self, Self::Error> {
        let hash: [u8; 32] = run_genesis_request
            .get_genesis_config_hash()
            .try_into()
            .map_err(|_| MappingError::TryFromSlice)?;
        Ok(RunGenesisRequest::new(
            hash.into(),
            run_genesis_request.take_protocol_version().into(),
            run_genesis_request.take_ee_config().try_into()?,
        ))
    }
}

impl From<RunGenesisRequest> for ipc::RunGenesisRequest {
    fn from(run_genesis_request: RunGenesisRequest) -> ipc::RunGenesisRequest {
        let mut res = ipc::RunGenesisRequest::new();
        res.set_genesis_config_hash(run_genesis_request.genesis_config_hash().value().to_vec());
        res.set_protocol_version(run_genesis_request.protocol_version().into());
        res.set_ee_config(run_genesis_request.take_ee_config().into());
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_server::mappings::test_utils;

    #[test]
    fn round_trip() {
        let run_genesis_request = rand::random();
        test_utils::protobuf_round_trip::<RunGenesisRequest, ipc::RunGenesisRequest>(
            run_genesis_request,
        );
    }
}
