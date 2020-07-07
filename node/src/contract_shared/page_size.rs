use std::io;

use libc::{c_long, sysconf, _SC_PAGESIZE};

/// Returns OS page size
pub fn get_page_size() -> Result<usize, io::Error> {
    // https://www.gnu.org/software/libc/manual/html_node/Sysconf.html
    let value: c_long = unsafe { sysconf(_SC_PAGESIZE) };

    if value < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(value as usize)
}
