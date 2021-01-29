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

/// Returns system module for the given TrackingCopy and Preprocessor instances.
/// If not present, returns precondition failure.
macro_rules! system_module {
    ($tracking_copy: ident, $preprocessor: expr) => {{
        match $tracking_copy.borrow_mut().get_system_module($preprocessor) {
            Ok(module) => module,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        }
    }};
}

/// Returns a contract under specified hash.
/// If not present, returns precondition failure.
macro_rules! get_contract {
    ($tracking_copy: ident, $contract_hash: expr, $correlation_id: ident) => {{
        match $tracking_copy
            .borrow_mut()
            .get_contract($correlation_id, $contract_hash)
        {
            Ok(contract) => contract,
            Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
        }
    }};
}

/// Matches on the result and returns early with en error if it's not valid.
macro_rules! fail_precondition {
    ($result: expr) => {{
        match $result {
            Ok(value) => value,
            Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
        }
    }};
    ($maybe: expr, $cause: expr) => {{
        match $maybe {
            Some(value) => value,
            None => return Ok(ExecutionResult::precondition_failure($cause)),
        }
    }};
}
