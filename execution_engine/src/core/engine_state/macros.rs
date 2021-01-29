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

/// Returns tracking copy at specific state hash.
macro_rules! tracking_copy {
    ($self: expr, $prestate_hash: ident) => {{
        match $self.tracking_copy($prestate_hash) {
            Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            Ok(None) => return Err(RootNotFound::new($prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        }
    }};
}
