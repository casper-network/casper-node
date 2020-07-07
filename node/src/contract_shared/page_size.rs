use std::io;

use lazy_static::lazy_static;
use libc::{c_long, sysconf, _SC_PAGESIZE};
use serde::{Deserialize, Deserializer};
use tracing::warn;

/// Sensible default for many if not all systems.
const DEFAULT_PAGE_SIZE: usize = 4096;

lazy_static! {
    pub static ref PAGE_SIZE: usize = get_page_size().unwrap_or(DEFAULT_PAGE_SIZE);
}

/// Returns OS page size
fn get_page_size() -> Result<usize, io::Error> {
    // https://www.gnu.org/software/libc/manual/html_node/Sysconf.html
    let value: c_long = unsafe { sysconf(_SC_PAGESIZE) };

    if value < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(value as usize)
}

/// Deserializes a `usize` but warns if it is not a multiple of the OS page size.
pub fn deserialize_page_size_multiple<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value % *PAGE_SIZE != 0 {
        warn!(
            "maximum size {} is not multiple of system page size {}",
            value, *PAGE_SIZE,
        );
    }

    Ok(value)
}
