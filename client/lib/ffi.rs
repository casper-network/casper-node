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

/// Private macro for parsing arguments from c strings, (const char *, or *const c_char in rust
/// terms). The sad path contract here is that we indicate there was an error by returning `false`,
/// then we store the argument -name- as an Error::InvalidArgument in LAST_ERROR. The happy path is
/// left up to callsites to define.
macro_rules! r#try_unsafe_arg {
    ($arg:expr) => {{
        let result = unsafe_str_arg($arg, stringify!($arg));
        try_unwrap_result!(result)
    }};
}

/// Private macro for unwrapping a result value or setting an appropriate error and returning
/// early with `false` to indicate it's existence.
macro_rules! r#try_unwrap_result {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => {
                set_last_error(error);
                return false;
            }
        }
    };
}

/// Private macro for unwrapping an optional value or setting an appropriate error and returning
/// early with `false` to indicate it's existence.
macro_rules! r#try_unwrap_option {
    ($arg:expr, or_else => $err:expr) => {
        match $arg {
            Some(value) => value,
            None => {
                set_last_error($err);
                return false;
            }
        }
    };
}

/// Private macro for unwrapping our internal json-rpcs or, optionally, storing the error in
/// LAST_ERROR and returning `false` to indicate that an error has occurred. Similar to
/// `try_unsafe_arg!`, this handles the sad path, and the happy path is left up to callsites.
macro_rules! r#try_unwrap_rpc {
    ($rpc:expr) => {{
        let rpc = try_unwrap_result!($rpc);
        let rpc_result = try_unwrap_option!(rpc.get_result(), or_else => {
            let rpc_err = rpc.get_error().expect("should be error");
            Error::ResponseIsError(rpc_err.to_owned())
        });
        try_unwrap_result!(serde_json::to_string(&rpc_result).map_err(Into::into))
    }};
}

/// Private macro to wrap TryInto implementing types with a human-readable error message describing
/// the field name at the callsite.
macro_rules! r#try_arg_into {
    ($arg:expr) => {{
        try_unwrap_result!(unsafe_try_into($arg, stringify!($arg)))
    }};
}

fn unsafe_str_arg(arg: *const c_char, arg_name: &'static str) -> Result<&'static str> {
    unsafe {
        // Strings are never required to be passed at this level, instead we return "" if the ptr ==
        // null and let the library deal with parsing values.
        if arg.is_null() {
            return Ok(Default::default());
        }
        CStr::from_ptr(arg).to_str()
    }
    .map_err(|error| {
        Error::InvalidArgument(
            arg_name,
            format!(
                "invalid utf8 value passed for arg '{}': {:?}",
                stringify!($arg),
                error,
            ),
        )
    })
}

fn unsafe_vec_of_str_arg(
    arg: *const *const c_char,
    len: usize,
    arg_name: &'static str,
) -> Result<Vec<&'static str>> {
    let slice = unsafe { slice::from_raw_parts(arg, len) };
    let mut vec = Vec::with_capacity(len);
    for bytes in slice {
        // While null-ptr strings are usually allowed as single arguments, an array of strings
        // required to not contain null values.
        if bytes.is_null() {
            return Err(Error::FFIPtrNullButRequired(arg_name));
        }
        vec.push(unsafe_str_arg(*bytes, arg_name)?);
    }
    Ok(vec)
}

/// Helper to call TryInto::try_into on a *const ptr of our rust type implementing it.
/// This is used for
fn unsafe_try_into<T, I>(value: *const I, field_name: &'static str) -> Result<T>
where
    I: Clone,
    I: TryInto<T, Error = Error>,
{
    if value.is_null() {
        Err(Error::FFIPtrNullButRequired(field_name))
    } else {
        let value: T = unsafe { (*value).clone().try_into()? };
        Ok(value)
    }
}

/// Copy the contents of `strval` to a user-provided buffer.
///
/// `strval` is the rust `&str` utf8 string to copy.
/// `buf` is the caller-provided buffer to write into.
/// `len` is the size of the buffer `buf` in bytes.
/// - returns the number of bytes written to `buf`.
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

/// Perform a clean shutdown of resources gathered in the client library.
#[no_mangle]
pub extern "C" fn casper_shutdown_client() {
    let mut runtime = RUNTIME.lock().expect("should lock");
    *runtime = None; // triggers drop on our runtime
}

/// Gets the last error copied to the provided buffer.
///
/// * `buf` is the buffer where the result will be stored.
/// * `len` is the length of the `buf` buffer in bytes.
/// - returns the number of bytes written to `buf`.
#[no_mangle]
pub extern "C" fn casper_get_last_error(buf: *mut c_uchar, len: usize) -> usize {
    if let Some(last_err) = &*LAST_ERROR.lock().expect("should lock") {
        let err_str = format!("{}", last_err);
        return copy_str_to_buf(&err_str, buf, len);
    }
    0
}

/// Creates a `Deploy` and sends it to the network for execution.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let deploy_params = try_arg_into!(deploy_params);
    let session_params = try_arg_into!(session_params);
    let payment_params = try_arg_into!(payment_params);
    runtime.block_on(async move {
        let result = super::put_deploy(
            maybe_rpc_id,
            node_address,
            verbose,
            deploy_params,
            session_params,
            payment_params,
        );
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Creates a `Deploy` and outputs it to a file or stdout.
///
/// See [super::make_deploy](function.make_deploy.html) for more details
#[no_mangle]
pub extern "C" fn casper_make_deploy(
    maybe_output_path: *const c_char,
    deploy_params: *const casper_deploy_params_t,
    session_params: *const casper_session_params_t,
    payment_params: *const casper_payment_params_t,
) -> bool {
    let maybe_output_path = try_unsafe_arg!(maybe_output_path);
    let deploy_params = try_arg_into!(deploy_params);
    let session_params = try_arg_into!(session_params);
    let payment_params = try_arg_into!(payment_params);
    let result = super::make_deploy(
        maybe_output_path,
        deploy_params,
        session_params,
        payment_params,
    );
    try_unwrap_result!(result);
    true
}

/// Reads a previously-saved `Deploy` from a file, cryptographically signs it, and outputs it to a
/// file or stdout.
///
/// See [super::sign_deploy_file](function.sign_deploy_file.html) for more details.
#[no_mangle]
pub extern "C" fn casper_sign_deploy_file(
    input_path: *const c_char,
    secret_key: *const c_char,
    maybe_output_path: *const c_char,
) -> bool {
    let input_path = try_unsafe_arg!(input_path);
    let secret_key = try_unsafe_arg!(secret_key);
    let maybe_output_path = try_unsafe_arg!(maybe_output_path);
    let result = super::sign_deploy_file(input_path, secret_key, maybe_output_path);
    try_unwrap_result!(result);
    true
}

/// Reads a previously-saved `Deploy` from a file and sends it to the network for execution.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let input_path = try_unsafe_arg!(input_path);
    runtime.block_on(async move {
        let result = super::send_deploy_file(maybe_rpc_id, node_address, verbose, input_path);
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Transfers funds between purses.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let amount = try_unsafe_arg!(amount);
    let maybe_source_purse = try_unsafe_arg!(maybe_source_purse);
    let maybe_target_purse = try_unsafe_arg!(maybe_target_purse);
    let maybe_target_account = try_unsafe_arg!(maybe_target_account);
    let deploy_params = try_arg_into!(deploy_params);
    let payment_params = try_arg_into!(payment_params);
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
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Retrieves a `Deploy` from the network.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let deploy_hash = try_unsafe_arg!(deploy_hash);
    runtime.block_on(async move {
        let result = super::get_deploy(maybe_rpc_id, node_address, verbose, deploy_hash);
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Retrieves a `Block` from the network.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let maybe_block_id = try_unsafe_arg!(maybe_block_id);
    runtime.block_on(async move {
        let result = super::get_block(maybe_rpc_id, node_address, verbose, maybe_block_id);
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Retrieves a state root hash at a given `Block`.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let maybe_block_id = try_unsafe_arg!(maybe_block_id);
    runtime.block_on(async move {
        let result =
            super::get_state_root_hash(maybe_rpc_id, node_address, verbose, maybe_block_id);
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Retrieves a stored value from the network.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let state_root_hash = try_unsafe_arg!(state_root_hash);
    let key = try_unsafe_arg!(key);
    let path = try_unsafe_arg!(path);
    runtime.block_on(async move {
        let result = super::get_item(
            maybe_rpc_id,
            node_address,
            verbose,
            state_root_hash,
            key,
            path,
        );
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Retrieves a purse's balance from the network.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    let state_root_hash = try_unsafe_arg!(state_root_hash);
    let purse = try_unsafe_arg!(purse);
    runtime.block_on(async move {
        let result =
            super::get_balance(maybe_rpc_id, node_address, verbose, state_root_hash, purse);
        let response = try_unwrap_rpc!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    })
}

/// Retrieves the bids and validators as of the most recently added `Block`.
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
    let runtime = try_unwrap_option!(&mut *runtime, or_else => Error::FFISetupNotCalled);
    let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
    let node_address = try_unsafe_arg!(node_address);
    runtime.block_on(async move {
        let result = super::get_auction_info(maybe_rpc_id, node_address, verbose);
        let response = try_unwrap_rpc!(result);
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
        let secret_key = unsafe_str_arg(self.secret_key, "casper_deploy_params_t.secret_key")?;
        let timestamp = unsafe_str_arg(self.timestamp, "casper_deploy_params_t.timestamp")?;
        let ttl = unsafe_str_arg(self.ttl, "casper_deploy_params_t.ttl")?;
        let gas_price = unsafe_str_arg(self.gas_price, "casper_deploy_params_t.gas_price")?;
        let chain_name = unsafe_str_arg(self.chain_name, "casper_deploy_params_t.chain_name")?;
        let dependencies = unsafe_vec_of_str_arg(
            self.dependencies,
            self.dependencies_len,
            "casper_deploy_params_t.dependencies",
        )?;
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
        let payment_amount = unsafe_str_arg(
            self.payment_amount,
            "casper_payment_params_t.payment_amount",
        )?;
        let payment_hash =
            unsafe_str_arg(self.payment_hash, "casper_payment_params_t.payment_hash")?;
        let payment_name =
            unsafe_str_arg(self.payment_name, "casper_payment_params_t.payment_name")?;
        let payment_package_hash = unsafe_str_arg(
            self.payment_package_hash,
            "casper_payment_params_t.payment_package_hash",
        )?;
        let payment_package_name = unsafe_str_arg(
            self.payment_package_name,
            "casper_payment_params_t.payment_package_name",
        )?;
        let payment_path =
            unsafe_str_arg(self.payment_path, "casper_payment_params_t.payment_path")?;
        let payment_args_simple = unsafe_vec_of_str_arg(
            self.payment_args_simple,
            self.payment_args_simple_len,
            "caser_payment_params_t.payment_args_simple",
        )?;
        let payment_args_complex = unsafe_str_arg(
            self.payment_args_complex,
            "casper_payment_params_t.payment_args_complex",
        )?;
        let payment_version = unsafe_str_arg(
            self.payment_version,
            "casper_payment_params_t.payment_version",
        )?;
        let payment_entry_point = unsafe_str_arg(
            self.payment_entry_point,
            "casper_payment_params_t.payment_entry_point",
        )?;
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
        let session_hash =
            unsafe_str_arg(self.session_hash, "casper_session_params_t.session_hash")?;
        let session_name =
            unsafe_str_arg(self.session_name, "casper_session_params_t.session_name")?;
        let session_package_hash = unsafe_str_arg(
            self.session_package_hash,
            "casper_session_params_t.sessio_package_hash",
        )?;
        let session_package_name = unsafe_str_arg(
            self.session_package_name,
            "casper_session_params_t.session_package_name",
        )?;
        let session_path =
            unsafe_str_arg(self.session_path, "casper_session_params_t.session_path")?;
        let session_args_simple = unsafe_vec_of_str_arg(
            self.session_args_simple,
            self.session_args_simple_len,
            "casper_session_params_t.session_args_simple",
        )?;
        let session_args_complex = unsafe_str_arg(
            self.session_args_complex,
            "casper_session_params_t.session_args_complex",
        )?;
        let session_version = unsafe_str_arg(
            self.session_version,
            "casper_session_params_t.session_version",
        )?;
        let session_entry_point = unsafe_str_arg(
            self.session_entry_point,
            "casper_session_params_t.session_entry_point",
        )?;
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
