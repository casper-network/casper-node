use casper_types::{api_error, bytesrepr::ToBytes, ApiError, Key};

extern "C" {
    fn casper_control_management(key_ptr: *const u8, key_size: usize, operation: u32) -> i32;
}

/// Enable or disable an entity refered by a [`Key`].
///
/// This function currenly works for [`Key::Account`] variants.
pub(crate) fn control_management(key: Key, operation: bool) -> Result<(), ApiError> {
    let key_bytes = key.to_bytes()?;
    let ret_value =
        unsafe { casper_control_management(key_bytes.as_ptr(), key_bytes.len(), operation as u32) };
    api_error::result_from(ret_value)
}
