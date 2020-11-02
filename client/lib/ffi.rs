//! Foreign function interfaces.

use std::{
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
    static ref RUNTIME: Mutex<runtime::Runtime> = Mutex::new(runtime::Runtime::new().expect("should build tokio runtime"));
}

fn set_last_error(error: Error) {
    let last_error = &mut *LAST_ERROR.lock().expect("should lock");
    *last_error = Some(error)
}

/// Maximum length of error-string output in bytes.
pub const MAX_ERROR_LEN: usize = 255;

/// Maximum length of provided `response_buf` buffer.
pub const MAX_RESPONSE_BUFFER_LEN: usize = 1024;

fn unsafe_arg(arg: *const c_char) -> Result<&'static str> {
    unsafe { CStr::from_ptr(arg).to_str() }.map_err(|error| {
        Error::InvalidArgument(format!(
            "invalid utf8 value passed for arg {}: {:?}",
            stringify!($arg),
            error,
        ))
    })
}

/// Private macro for parsing aruments from c strings, (const char *, or *const c_char in rust
/// terms). The sad path contract here is that we indicate there was an error by returning `false`,
/// then we store the argument -name- as an Error::InvalidArgument in LAST_ERROR. The happy path is
/// left up to callsites to define.
macro_rules! r#try_unsafe_arg {
    ($arg:expr) => {
        match unsafe_arg($arg) {
            Ok(value) => value,
            Err(error) => {
                set_last_error(error);
                return false;
            }
        }
    };
}

/// Private macro for unwrapping results or, optionally, storing the error in LAST_ERROR and
/// returning `false` to indicate that an error has occurred. Similar to `try_arg!`, this handles
/// the sad path, and the happy path is left up to callsites.
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

/// Copy the contents of `strval` to a user-provided buffer.
fn copy_str_to_buf(strval: &str, buf: *mut c_uchar, len: usize) -> usize {
    let mut_buf = unsafe { slice::from_raw_parts_mut::<u8>(buf, len) };
    let lesser_len = len.min(strval.len());
    let strval = strval.as_bytes();
    mut_buf[0..lesser_len].clone_from_slice(&strval[0..lesser_len]);
    len
}

/// Get the last error copied to the provided buffer (must be large enough, TODO: 255 chars?
/// MAX_ERROR_LEN)
#[no_mangle]
pub extern "C" fn get_last_error(buf: *mut c_uchar, len: usize) -> usize {
    if let Some(last_err) = &*LAST_ERROR.lock().expect("should unlock") {
        let err_str = format!("{}", last_err);
        return copy_str_to_buf(&err_str, buf, len);
    }
    0
}

/// Get auction info.
///
/// See [super::get_auction_info](function.get_auction_info.html) for more details.
#[no_mangle]
pub extern "C" fn get_auction_info(
    maybe_rpc_id: *const c_char,
    node_address: *const c_char,
    verbose: bool,
    response_buf: *mut c_uchar,
    response_buf_len: usize,
) -> bool {
    let ret = RUNTIME.lock().expect("should lock").block_on(async move {
        let maybe_rpc_id = try_unsafe_arg!(maybe_rpc_id);
        let node_address = try_unsafe_arg!(node_address);
        let result = super::get_auction_info(maybe_rpc_id, node_address, verbose);
        let response = try_rpc_str!(result);
        copy_str_to_buf(&response, response_buf, response_buf_len);
        true
    });
    ret
}
