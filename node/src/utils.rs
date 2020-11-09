//! Various functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

mod external;
mod median;
pub mod milliseconds;
mod round_robin;

use std::{
    cell::RefCell,
    fmt::{self, Display, Formatter},
    fs, io,
    net::{SocketAddr, ToSocketAddrs},
    path::{Path, PathBuf},
};

use datasize::DataSize;
use lazy_static::lazy_static;
use libc::{c_long, sysconf, _SC_PAGESIZE};
use thiserror::Error;

#[cfg(test)]
pub use external::RESOURCES_PATH;
pub use external::{External, LoadError, Loadable};
pub(crate) use median::weighted_median;
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

/// Parses a network address from a string, with DNS resolution.
pub(crate) fn resolve_address(addr: &str) -> io::Result<SocketAddr> {
    addr.to_socket_addrs()?.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("could not resolve `{}`", addr),
        )
    })
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
#[error("could not read '{0}': {error}", .path.display())]
pub struct ReadFileError {
    /// Path that failed to be read.
    path: PathBuf,
    /// The underlying OS error.
    #[source]
    error: io::Error,
}

/// Error writing a file
#[derive(Debug, Error)]
#[error("could not write to '{0}': {error}", .path.display())]
pub struct WriteFileError {
    /// Path that failed to be written to.
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

/// Write data to `path`.
///
/// Wraps `fs::write`, but preserves the filename for better error printing.
pub(crate) fn write_file<P: AsRef<Path>, B: AsRef<[u8]>>(
    filename: P,
    data: B,
) -> Result<(), WriteFileError> {
    let path = filename.as_ref();
    fs::write(path, data.as_ref()).map_err(|error| WriteFileError {
        path: path.to_owned(),
        error,
    })
}

/// With-directory context.
///
/// Associates a type with a "working directory".
#[derive(Clone, DataSize, Debug)]
pub struct WithDir<T> {
    dir: PathBuf,
    value: T,
}

impl<T> WithDir<T> {
    /// Creates a new with-directory context.
    pub fn new<P: Into<PathBuf>>(path: P, value: T) -> Self {
        WithDir {
            dir: path.into(),
            value,
        }
    }

    /// Returns a reference to the inner path.
    pub(crate) fn dir(&self) -> &Path {
        self.dir.as_ref()
    }

    /// Deconstructs a with-directory context.
    pub(crate) fn into_parts(self) -> (PathBuf, T) {
        (self.dir, self.value)
    }

    pub(crate) fn map_ref<U, F: FnOnce(&T) -> U>(&self, f: F) -> WithDir<U> {
        WithDir {
            dir: self.dir.clone(),
            value: f(&self.value),
        }
    }

    /// Get a reference to the inner value.
    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    /// Adds `self.dir` as a parent if `path` is relative, otherwise returns `path` unchanged.
    pub(crate) fn with_dir(&self, path: PathBuf) -> PathBuf {
        if path.is_relative() {
            self.dir.join(path)
        } else {
            path
        }
    }
}

/// The source of a piece of data.
#[derive(Clone, Debug)]
pub enum Source<I> {
    /// A peer with the wrapped ID.
    Peer(I),
    /// A client.
    Client,
}

impl<I: Clone> Source<I> {
    /// If `self` represents a peer, returns its ID, otherwise returns `None`.
    pub(crate) fn node_id(&self) -> Option<I> {
        match self {
            Source::Peer(node_id) => Some(node_id.clone()),
            Source::Client => None,
        }
    }
}

impl<I: Display> Display for Source<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Source::Peer(node_id) => Display::fmt(node_id, formatter),
            Source::Client => write!(formatter, "client"),
        }
    }
}
