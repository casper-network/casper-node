/// Returns a ProtocolData instance that's valid for specific protocol version.
macro_rules! protocol_data {
    ($state: expr, $protocol_version: ident) => {{
        match $state.get_protocol_data($protocol_version) {
            Ok(Some(protocol_data)) => protocol_data,
            Ok(None) => {
                let error = Error::InvalidProtocolVersion($protocol_version);
                return Ok(ExecutionResult::precondition_failure(error));
            }
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(Error::Exec(
                    error.into(),
                )));
            }
        }
    }};
}
