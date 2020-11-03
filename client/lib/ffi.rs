//! Foreign function interfaces.

use std::{
    convert::TryInto,
    ffi::CStr,
    os::raw::{c_char, c_uchar},
    slice,
    sync::Mutex,
};

use lazy_static::lazy_static;
use tokio::runtime;

use super::error::{Error, Result};

lazy_static! {
    static ref LAST_ERROR: Mutex<Option<Error>> = Mutex::new(None);
    static ref RUNTIME: Mutex<Option<runtime::Runtime>> = Mutex::new(None);
}

fn set_last_error(error: Error) {
    let last_error = &mut *LAST_ERROR.lock().expect("should lock");
    *last_error = Some(error)
}

/// Maximum length of error-string output in bytes.
pub const CASPER_MAX_ERROR_LEN: usize = 255;

/// Maximum length of provided `response_buf` buffer.
pub const CASPER_MAX_RESPONSE_BUFFER_LEN: usize = 1024;

fn unsafe_str_arg(arg: *const c_char, required: bool) -> Result<&'static str> {
    unsafe {
        if arg.is_null() {
            if required {
                return Err(Error::FFIPtrNullButRequired);
            } else {
                return Ok(Default::default());
            }
        }
        CStr::from_ptr(arg).to_str()
    }
    .map_err(|error| {
        Error::InvalidArgument(format!(
            "invalid utf8 value passed for arg {}: {:?}",
            stringify!($arg),
            error,
        ))
    })
}

fn unsafe_vec_of_str_arg(arg: *const *const c_char, len: usize) -> Result<Vec<&'static str>> {
    let slice = unsafe { slice::from_raw_parts(arg, len) };
    let mut vec = Vec::with_capacity(len);
    for bytes in slice {
        vec.push(unsafe_str_arg(*bytes, true)?);
    }
    Ok(vec)
}

fn unsafe_try_into<T, I>(value: *const I) -> Result<T>
where
    I: Clone,
    I: TryInto<T, Error = Error>,
{
    if value.is_null() {
        let value: T = unsafe { (*value).clone().try_into()? };
        Ok(value)
    } else {
        Err(Error::FFIPtrNullButRequired)
    }
}

/// Private macro for parsing aruments from c strings, (const char *, or *const c_char in rust
/// terms). The sad path contract here is that we indicate there was an error by returning `false`,
/// then we store the argument -name- as an Error::InvalidArgument in LAST_ERROR. The happy path is
/// left up to callsites to define.
macro_rules! r#try_unwrap {
    ($arg:expr) => {
        match $arg {
            Ok(value) => value,
            Err(error) => {
                set_last_error(error);
                return false;
            }
        }
    };
}

/// Private macro for unwrapping our internal json-rpcs or, optionally, storing the error in
/// LAST_ERROR and returning `false` to indicate that an error has occurred. Similar to
/// `try_unsafe_arg!`, this handles the sad path, and the happy path is left up to callsites.
macro_rules! r#try_rpc_str {
    ($rpc:expr) => {
        match $rpc {
            Ok(value) => match value.get_result() {
                Some(value) => match serde_json::to_string(value) {
                    Ok(value) => value,
                    Err(serde_error) => {
                        set_last_error(serde_error.into());
                        return false;
                    }
                },
                None => {
                    let rpc_err = value.get_error().expect("should be error");
                    set_last_error(Error::ResponseIsError(rpc_err.to_owned()));
                    return false;
                }
            },
            Err(error) => {
                set_last_error(error);
                return false;
            }
        }
    };
}

/// Private macro for unwrapping an optional value or setting an appropriate error and returning
/// early with `false` to indicate it's existence.
macro_rules! r#try_option_or {
    ($arg:expr, $err:expr) => {
        match $arg {
            Some(value) => value,
            None => {
                set_last_error($err);
                return false;
            }
        }
    };
}

/// Copy the contents of `strval` to a user-provided buffer.
fn copy_str_to_buf(strval: &str, buf: *mut c_uchar, len: usize) -> usize {
    let mut_buf = unsafe { slice::from_raw_parts_mut::<u8>(buf, len) };
    let lesser_len = len.min(strval.len());
    let strval = strval.as_bytes();
    mut_buf[0..lesser_len].clone_from_slice(&strval[0..lesser_len]);
    len
}

/// Perform needed setup for the client library.
#[no_mangle]
pub extern "C" fn casper_setup_client() {
    let mut runtime = RUNTIME.lock().expect("should lock");
    // TODO: runtime opts
    *runtime = Some(runtime::Runtime::new().expect("should create tokio runtime"));
}

/// Perform shutdown of resources gathered in the client library.
#[no_mangle]
pub extern "C" fn casper_shutdown_client() {
    let mut runtime = RUNTIME.lock().expect("should lock");
    *runtime = None; // triggers drop on our runtime
}

/// Get the last error copied to the provided buffer (must be large enough, TODO: 255 chars?
/// MAX_ERROR_LEN)
#[no_mangle]
pub extern "C" fn casper_get_last_error(buf: *mut c_uchar, len: usize) -> usize {
    if let Some(last_err) = &*LAST_ERROR.lock().expect("should lock") {
        let err_str = format!("{}", last_err);
        return copy_str_to_buf(&err_str, buf, len);
    }
    0
}

/// Put deploy.
///
/// See [super::put_deploy](function.put_deploy.html) for more details
#[no_mangle]
pub extern "C" fn casper_put_deploy(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    deploy_params: *const casper_deploy_params_t,
    session_params: *const casper_session_params_t,
    payment_params: *const casper_payment_params_t,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let deploy_params = try_unwrap!(unsafe_try_into(deploy_params));
    let session_params = try_unwrap!(unsafe_try_into(session_params));
    let payment_params = try_unwrap!(unsafe_try_into(payment_params));
    runtime.block_on(async move {
        let result = super::put_deploy(
            maybe_rpc_id,
            node_address,
            verbose,
            deploy_params,
            session_params,
            payment_params,
        );
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Make deploy.
///
/// See [super::make_deploy](function.make_deploy.html) for more details
#[no_mangle]
pub extern "C" fn casper_make_deploy(
    maybe_output_path: *const c_char,
    deploy_params: *const casper_deploy_params_t,
    session_params: *const casper_session_params_t,
    payment_params: *const casper_payment_params_t,
) -> bool {
    let maybe_output_path = try_unwrap!(unsafe_str_arg(maybe_output_path, false));
    let deploy_params = try_unwrap!(unsafe_try_into(deploy_params));
    let session_params = try_unwrap!(unsafe_try_into(session_params));
    let payment_params = try_unwrap!(unsafe_try_into(payment_params));
    let result = super::make_deploy(
        maybe_output_path,
        deploy_params,
        session_params,
        payment_params,
    );
    try_unwrap!(result);
    true
}

/// Sign deploy file.
///
/// See [super::sign_deploy_file](function.sign_deploy_file.html) for more details.
#[no_mangle]
pub extern "C" fn casper_sign_deploy_file(
    input_path: *const c_char,
    secret_key: *const c_char,
    maybe_output_path: *const c_char,
) -> bool {
    let input_path = try_unwrap!(unsafe_str_arg(input_path, false));
    let secret_key = try_unwrap!(unsafe_str_arg(secret_key, false));
    let maybe_output_path = try_unwrap!(unsafe_str_arg(maybe_output_path, false));
    let result = super::sign_deploy_file(input_path, secret_key, maybe_output_path);
    try_unwrap!(result);
    true
}

/// Send deploy file.
///
/// See [super::send_deploy_file](function.send_deploy_file.html) for more details.
#[no_mangle]
pub extern "C" fn casper_send_deploy_file(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    input_path: *const c_char,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let input_path = try_unwrap!(unsafe_str_arg(input_path, false));
    runtime.block_on(async move {
        let result = super::send_deploy_file(maybe_rpc_id, node_address, verbose, input_path);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Transfer.
///
/// See [super::transfer](function.transfer.html) for more details
#[no_mangle]
pub extern "C" fn casper_transfer(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    amount: *const c_char,
    maybe_source_purse: *const c_char,
    maybe_target_purse: *const c_char,
    maybe_target_account: *const c_char,
    deploy_params: *const casper_deploy_params_t,
    payment_params: *const casper_payment_params_t,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let amount = try_unwrap!(unsafe_str_arg(amount, false));
    let maybe_source_purse = try_unwrap!(unsafe_str_arg(maybe_source_purse, false));
    let maybe_target_purse = try_unwrap!(unsafe_str_arg(maybe_target_purse, false));
    let maybe_target_account = try_unwrap!(unsafe_str_arg(maybe_target_account, false));
    let deploy_params = try_unwrap!(unsafe_try_into(deploy_params));
    let payment_params = try_unwrap!(unsafe_try_into(payment_params));
    runtime.block_on(async move {
        let result = super::transfer(
            maybe_rpc_id,
            node_address,
            verbose,
            amount,
            maybe_source_purse,
            maybe_target_purse,
            maybe_target_account,
            deploy_params,
            payment_params,
        );
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Get deploy.
///
/// See [super::get_deploy](function.get_deploy.html) for more details.
#[no_mangle]
pub extern "C" fn casper_get_deploy(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    deploy_hash: *const c_char,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let deploy_hash = try_unwrap!(unsafe_str_arg(deploy_hash, false));
    runtime.block_on(async move {
        let result = super::get_deploy(maybe_rpc_id, node_address, verbose, deploy_hash);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Get block.
///
/// See [super::get_block](function.get_block.html) for more details.
#[no_mangle]
pub extern "C" fn casper_get_block(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    maybe_block_id: *const c_char,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let maybe_block_id = try_unwrap!(unsafe_str_arg(maybe_block_id, false));
    runtime.block_on(async move {
        let result = super::get_block(maybe_rpc_id, node_address, verbose, maybe_block_id);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Get state root hash.
///
/// See [super::get_state_root_hash](function.get_state_root_hash.html) for more details.
#[no_mangle]
pub extern "C" fn casper_get_state_root_hash(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    maybe_block_id: *const c_char,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let maybe_block_id = try_unwrap!(unsafe_str_arg(maybe_block_id, false));
    runtime.block_on(async move {
        let result =
            super::get_state_root_hash(maybe_rpc_id, node_address, verbose, maybe_block_id);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Get item.
///
/// See [super::get_item](function.get_item.html) for more details.
#[no_mangle]
pub extern "C" fn casper_get_item(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    state_root_hash: *const c_char,
    key: *const c_char,
    path: *const c_char,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let state_root_hash = try_unwrap!(unsafe_str_arg(state_root_hash, false));
    let key = try_unwrap!(unsafe_str_arg(key, false));
    let path = try_unwrap!(unsafe_str_arg(path, false));
    runtime.block_on(async move {
        let result = super::get_item(
            maybe_rpc_id,
            node_address,
            verbose,
            state_root_hash,
            key,
            path,
        );
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Get balance.
///
/// See [super::get_balance](function.get_balance.html) for more details.
#[no_mangle]
pub extern "C" fn casper_get_balance(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    state_root_hash: *const c_char,
    purse: *const c_char,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    let state_root_hash = try_unwrap!(unsafe_str_arg(state_root_hash, false));
    let purse = try_unwrap!(unsafe_str_arg(purse, false));
    runtime.block_on(async move {
        let result =
            super::get_balance(maybe_rpc_id, node_address, verbose, state_root_hash, purse);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Get auction info.
///
/// See [super::get_auction_info](function.get_auction_info.html) for more details.
#[no_mangle]
pub extern "C" fn casper_get_auction_info(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let mut runtime = RUNTIME.lock().expect("should lock");
    let runtime = try_option_or!(&mut *runtime, Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unwrap!(unsafe_str_arg(maybe_rpc_id, false));
    let node_address = try_unwrap!(unsafe_str_arg(node_address, false));
    runtime.block_on(async move {
        let result = super::get_auction_info(maybe_rpc_id, node_address, verbose);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Container for `Deploy` construction options.
///
/// See [DeployStrParams](struct.DeployStrParams.html) for more info.
#[allow(non_snake_case)]
#[repr(C)]
#[derive(Clone)]
pub struct casper_deploy_params_t {
    secret_key: *const c_char,
    timestamp: *const c_char,
    ttl: *const c_char,
    gas_price: *const c_char,
    dependencies: *const *const c_char,
    dependencies_len: usize,
    chain_name: *const c_char,
}

impl TryInto<super::DeployStrParams<'_>> for casper_deploy_params_t {
    type Error = Error;

    fn try_into(self) -> Result<super::DeployStrParams<'static>> {
        let secret_key = unsafe_str_arg(self.secret_key, false)?;
        let timestamp = unsafe_str_arg(self.timestamp, false)?;
        let ttl = unsafe_str_arg(self.ttl, false)?;
        let gas_price = unsafe_str_arg(self.gas_price, false)?;
        let chain_name = unsafe_str_arg(self.chain_name, false)?;
        let dependencies = unsafe_vec_of_str_arg(self.dependencies, self.dependencies_len)?;
        Ok(super::DeployStrParams {
            secret_key,
            timestamp,
            ttl,
            gas_price,
            chain_name,
            dependencies,
        })
    }
}

/// Container for `Payment` construction options.
///
/// See [PaymentStrParams](struct.PaymentStrParams.html) for more info.
#[allow(non_snake_case)]
#[repr(C)]
#[derive(Clone)]
pub struct casper_payment_params_t {
    payment_amount: *const c_char,
    payment_hash: *const c_char,
    payment_name: *const c_char,
    payment_package_hash: *const c_char,
    payment_package_name: *const c_char,
    payment_path: *const c_char,
    payment_args_simple: *const *const c_char,
    payment_args_simple_len: usize,
    payment_args_complex: *const c_char,
    payment_version: *const c_char,
    payment_entry_point: *const c_char,
}

impl TryInto<super::PaymentStrParams<'static>> for casper_payment_params_t {
    type Error = Error;

    fn try_into(self) -> Result<super::PaymentStrParams<'static>> {
        let payment_amount = unsafe_str_arg(self.payment_amount, false)?;
        let payment_hash = unsafe_str_arg(self.payment_hash, false)?;
        let payment_name = unsafe_str_arg(self.payment_name, false)?;
        let payment_package_hash = unsafe_str_arg(self.payment_package_hash, false)?;
        let payment_package_name = unsafe_str_arg(self.payment_package_name, false)?;
        let payment_path = unsafe_str_arg(self.payment_path, false)?;
        let payment_args_simple =
            unsafe_vec_of_str_arg(self.payment_args_simple, self.payment_args_simple_len)?;
        let payment_args_complex = unsafe_str_arg(self.payment_args_complex, false)?;
        let payment_version = unsafe_str_arg(self.payment_version, false)?;
        let payment_entry_point = unsafe_str_arg(self.payment_entry_point, false)?;
        Ok(super::PaymentStrParams {
            payment_amount,
            payment_hash,
            payment_name,
            payment_package_hash,
            payment_package_name,
            payment_path,
            payment_args_simple,
            payment_args_complex,
            payment_version,
            payment_entry_point,
        })
    }
}

/// Container for `Session` construction options.
///
/// See [SessionStrParams](struct.SessionStrParams.html) for more info.
#[allow(non_snake_case)]
#[repr(C)]
#[derive(Clone)]
pub struct casper_session_params_t {
    session_hash: *const c_char,
    session_name: *const c_char,
    session_package_hash: *const c_char,
    session_package_name: *const c_char,
    session_path: *const c_char,
    session_args_simple: *const *const c_char,
    session_args_simple_len: usize,
    session_args_complex: *const c_char,
    session_version: *const c_char,
    session_entry_point: *const c_char,
}

impl TryInto<super::SessionStrParams<'static>> for casper_session_params_t {
    type Error = Error;

    fn try_into(self) -> Result<super::SessionStrParams<'static>> {
        let session_hash = unsafe_str_arg(self.session_hash, false)?;
        let session_name = unsafe_str_arg(self.session_name, false)?;
        let session_package_hash = unsafe_str_arg(self.session_package_hash, false)?;
        let session_package_name = unsafe_str_arg(self.session_package_name, false)?;
        let session_path = unsafe_str_arg(self.session_path, false)?;
        let session_args_simple =
            unsafe_vec_of_str_arg(self.session_args_simple, self.session_args_simple_len)?;
        let session_args_complex = unsafe_str_arg(self.session_args_complex, false)?;
        let session_version = unsafe_str_arg(self.session_version, false)?;
        let session_entry_point = unsafe_str_arg(self.session_entry_point, false)?;
        Ok(super::SessionStrParams {
            session_hash,
            session_name,
            session_package_hash,
            session_package_name,
            session_path,
            session_args_simple,
            session_args_complex,
            session_version,
            session_entry_point,
        })
    }
}
