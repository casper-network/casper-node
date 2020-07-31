//! Various functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

mod round_robin;

use std::{
    cell::RefCell,
    fmt::{self, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
};

use lazy_static::lazy_static;
use libc::{c_long, sysconf, _SC_PAGESIZE};
use thiserror::Error;

pub(crate) use round_robin::WeightedRoundRobin;

/// Sensible default for many if not all systems.
const DEFAULT_PAGE_SIZE: usize = 4096;

lazy_static! {
    /// OS page size.
    pub static ref OS_PAGE_SIZE: usize = {
        // https://www.gnu.org/software/libc/manual/html_node/Sysconf.html
        let value: c_long = unsafe { sysconf(_SC_PAGESIZE) };
        if value <= 0 {
            DEFAULT_PAGE_SIZE
        } else {
            value as usize
        }
    };
}

/// Moves a value to the heap and then forgets about, leaving only a static reference behind.
#[inline]
pub(crate) fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

/// A display-helper that shows iterators display joined by ",".
#[derive(Debug)]
pub(crate) struct DisplayIter<T>(RefCell<Option<T>>);

impl<T> DisplayIter<T> {
    pub(crate) fn new(item: T) -> Self {
        DisplayIter(RefCell::new(Some(item)))
    }
}

impl<I, T> Display for DisplayIter<I>
where
    I: IntoIterator<Item = T>,
    T: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(src) = self.0.borrow_mut().take() {
            let mut first = true;
            for item in src.into_iter().take(f.width().unwrap_or(usize::MAX)) {
                if first {
                    first = false;
                    write!(f, "{}", item)?;
                } else {
                    write!(f, ", {}", item)?;
                }
            }

            Ok(())
        } else {
            write!(f, "DisplayIter:GONE")
        }
    }
}

/// Error reading a file.
#[derive(Debug, Error)]
#[error("could not read {0}: {error}", .path.display())]
pub struct ReadFileError {
    /// Path that failed to be read.
    path: PathBuf,
    /// The underlying OS error.
    #[source]
    error: io::Error,
}

/// Read complete at `path` into memory.
///
/// Wraps `fs::read`, but preserves the filename for better error printing.
pub fn read_file<P: AsRef<Path>>(filename: P) -> Result<Vec<u8>, ReadFileError> {
    let path = filename.as_ref();
    fs::read(path).map_err(|error| ReadFileError {
        path: path.to_owned(),
        error,
    })
}

/// Read a complete `path` into memory and convert to string.
///
/// Wraps `fs::read_to_string`, but preserves the filename for better error printing.
pub fn read_file_to_string<P: AsRef<Path>>(filename: P) -> Result<String, ReadFileError> {
    let path = filename.as_ref();
    fs::read_to_string(path).map_err(|error| ReadFileError {
        path: path.to_owned(),
        error,
    })
}
