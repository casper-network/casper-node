use bytes::Bytes;
use casper_executor_wasm_common::{
    error::{HOST_ERROR_INVALID_INPUT, HOST_ERROR_SUCCESS},
    flags::ReturnFlags,
};
use casper_executor_wasm_interface::{executor::ExecuteError, Caller, VMError, VMResult};
use casper_storage::global_state::GlobalStateReader;
use casper_types::{
    bytesrepr::{self, Bytes as BytesreprBytes},
    execution::RetValue,
};

use crate::context::Context;

pub(crate) fn host_return<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<u32> {
    let (flags, data) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (u32, Option<BytesreprBytes>)>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok(HOST_ERROR_INVALID_INPUT);
            }
        };
    let maybe_flags = ReturnFlags::from_bits(flags);
    let flags = match maybe_flags {
        Some(flags) => flags,
        None => {
            return Err(VMError::Execute(ExecuteError::ReturnFlagsNotSupported(
                flags,
            )))
        }
    };

    let data = data.map(|data| Bytes::from(data.take_inner()));
    if let Some(data) = &data {
        let key = caller.context().callee;
        let bytes = casper_types::bytesrepr::Bytes::from(data.to_vec());
        caller
            .context_mut()
            .tracking_copy
            .ret(key, RetValue::Bytes(bytes));
    }
    Err(VMError::Return { flags, data })
}

pub(crate) fn host_copy_input<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
) -> VMResult<(Option<Bytes>, u32)> {
    let input = caller.context().input.clone();

    Ok((Some(input), HOST_ERROR_SUCCESS))
}
