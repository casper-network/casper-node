//! Various functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

mod counting_channel;
pub mod ds;
mod external;
mod median;
pub mod milliseconds;
pub(crate) mod rlimit;
mod round_robin;

use std::{
    cell::RefCell,
    fmt::{self, Display, Formatter},
    fs::{self, File},
    io,
    net::{SocketAddr, ToSocketAddrs},
    num::ParseIntError,
    ops::{Add, Div},
    path::{Path, PathBuf},
    process,
};

use datasize::DataSize;
use fs2::FileExt;
use hyper::server::{conn::AddrIncoming, Builder, Server};
use io::{Read, Write};
use libc::{c_long, sysconf, _SC_PAGESIZE};
use once_cell::sync::Lazy;
use serde::Serialize;
use thiserror::Error;
use tracing::warn;

pub(crate) use counting_channel::{counting_unbounded_channel, CountingReceiver, CountingSender};
#[cfg(test)]
pub use external::RESOURCES_PATH;
pub use external::{External, LoadError, Loadable};
pub(crate) use median::weighted_median;
pub(crate) use round_robin::WeightedRoundRobin;

/// Sensible default for many if not all systems.
const DEFAULT_PAGE_SIZE: usize = 4096;

/// OS page size.
pub static OS_PAGE_SIZE: Lazy<usize> = Lazy::new(|| {
    // https://www.gnu.org/software/libc/manual/html_node/Sysconf.html
    let value: c_long = unsafe { sysconf(_SC_PAGESIZE) };
    if value <= 0 {
        DEFAULT_PAGE_SIZE
    } else {
        value as usize
    }
});

/// DNS resolution error.
#[derive(Debug, Error)]
#[error("could not resolve `{address}`: {kind}")]
pub struct ResolveAddressError {
    /// Address that failed to resolve.
    address: String,
    /// Reason for resolution failure.
    kind: ResolveAddressErrorKind,
}

/// DNS resolution error kind.
#[derive(Debug)]
enum ResolveAddressErrorKind {
    /// Resolve returned an error.
    ErrorResolving(io::Error),
    /// Resolution did not yield any address.
    NoAddressFound,
}

impl Display for ResolveAddressErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ResolveAddressErrorKind::ErrorResolving(err) => {
                write!(f, "could not run dns resolution: {}", err)
            }
            ResolveAddressErrorKind::NoAddressFound => {
                write!(f, "no addresses found")
            }
        }
    }
}

/// Parses a network address from a string, with DNS resolution.
pub(crate) fn resolve_address(address: &str) -> Result<SocketAddr, ResolveAddressError> {
    address
        .to_socket_addrs()
        .map_err(|err| ResolveAddressError {
            address: address.to_string(),
            kind: ResolveAddressErrorKind::ErrorResolving(err),
        })?
        .next()
        .ok_or_else(|| ResolveAddressError {
            address: address.to_string(),
            kind: ResolveAddressErrorKind::NoAddressFound,
        })
}

/// An error starting one of the HTTP servers.
#[derive(Debug, Error)]
pub enum ListeningError {
    /// Failed to resolve address.
    #[error("failed to resolve network address: {0}")]
    ResolveAddress(ResolveAddressError),

    /// Failed to listen.
    #[error("failed to listen on {address}: {error}")]
    Listen {
        /// The address attempted to listen on.
        address: SocketAddr,
        /// The failure reason.
        error: hyper::Error,
    },
}

pub(crate) fn start_listening(address: &str) -> Result<Builder<AddrIncoming>, ListeningError> {
    let address = resolve_address(address).map_err(|error| {
        warn!(%error, %address, "failed to start HTTP server, cannot parse address");
        ListeningError::ResolveAddress(error)
    })?;

    Server::try_bind(&address).map_err(|error| {
        warn!(%error, %address, "failed to start HTTP server");
        ListeningError::Listen { address, error }
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
#[derive(Clone, Debug, Serialize)]
pub enum Source<I> {
    /// A peer with the wrapped ID.
    Peer(I),
    /// A client.
    Client,
}

impl<I> Source<I> {
    pub(crate) fn from_client(&self) -> bool {
        matches!(self, Source::Client)
    }
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

/// Divides `numerator` by `denominator` and rounds to the closest integer.
pub(crate) fn div_round<T>(numerator: T, denominator: T) -> T
where
    T: Add<Output = T> + Div<Output = T> + From<u8> + Copy,
{
    (numerator + denominator / T::from(2)) / denominator
}

/// Used to unregister a metric from the Prometheus registry.
#[macro_export]
macro_rules! unregister_metric {
    ($registry:expr, $metric:expr) => {
        $registry
            .unregister(Box::new($metric.clone()))
            .unwrap_or_else(|_| {
                tracing::error!(
                    "unregistering {} failed: was not registered",
                    stringify!($metric)
                )
            });
    };

/// An error acquiring a pidfile.
#[derive(Debug, Error)]
enum PidfileError {
    /// The pidfile could not be opened at all.
    #[error("could not pidfile: {0}")]
    CouldNotOpen(#[source] io::Error),
    /// The pidfile could not be locked.
    #[error("could not lock pidfile: {0}")]
    LockFailed(#[source] io::Error),
    /// Error reading pidfile contents.
    #[error("reading existing pidfile failed: {0}")]
    ReadFailed(#[source] io::Error),
    /// Error writing pidfile contents.
    #[error("updating pidfile failed: {0}")]
    WriteFailed(#[source] io::Error),
    /// The pidfile was corrupted, its contents could not be read.
    #[error("corrupt pidfile")]
    Corrupted(ParseIntError),
    /// We encountere a previous PID, but it was the same as ours.
    ///
    /// This should never happen, as the exclusive lock should prevent it.
    #[error("read back our own pid from exlusively locked pidfile")]
    DuplicatedPid,
}

/// Pidfile outcome.
///
/// High-level description of the outcome of opening and locking the PIDfile.
#[must_use]
enum PidfileOutcome {
    /// Another instance of the node is likely running, or an attempt was made to reuse a pidfile.
    ///
    /// **Recommendation**: Exit to avoid resource conflicts.
    AnotherNodeRunning(PidfileError),
    /// The node crashed previously and could potentially have been corrupted.
    ///
    /// **Recommendation**: Run an integrity check, then potentially continue with intialization.
    ///                     **Store the `Pidfile`**.
    Crashed(Pidfile),
    /// Clean start, pidfile lock acquired.
    ///
    /// **Recommendation**: Continue with intialization, but **store the `Pidfile`**.
    Clean(Pidfile),
    /// There was an error managing the pidfile, not sure if we have crashed or not.
    ///
    /// **Recommendation**: Exit, as it will not be possible to determine a crash at the next
    /// start.
    PidfileError(PidfileError),
}

/// A PID file.
///
/// Records the current process' PID, removes it on exit. Can be used to determine whether or not
/// an application was shut down cleanly.
///
/// The pidfile is held open with an exclusive but advisory lock.
struct Pidfile {
    /// The pidfile.
    ///
    /// The file will be locked for the lifetime of `Pidfile`.
    pidfile: File,
    /// The pidfile location.
    path: PathBuf,
    /// Previous pidfile contents.
    previous: Option<u32>,
}

impl Pidfile {
    /// Acquire a `Pidfile` and give an actionale outcome.
    ///
    /// **Important**: This function should be called **before** opening whatever resources it is
    /// protecting.
    pub fn acquire<P: AsRef<Path>>(path: P) -> PidfileOutcome {
        match Pidfile::new(path) {
            Ok(pidfile) => {
                if pidfile.unclean_shutdown() {
                    PidfileOutcome::Crashed(pidfile)
                } else {
                    PidfileOutcome::Clean(pidfile)
                }
            }
            Err(err @ PidfileError::LockFailed(_)) => PidfileOutcome::AnotherNodeRunning(err),
            Err(err) => PidfileOutcome::PidfileError(err),
        }
    }

    /// Creates a new pidfile.
    ///
    /// The error-behavior of this function is important and can be used to distinguish between
    /// different conditions according to the table below. If the `Pidfile` is instantiated before
    /// the resource it is supposed to protect, the following actions are recommended:
    fn new<P: AsRef<Path>>(path: P) -> Result<Pidfile, PidfileError> {
        // First we try to open the pidfile, without disturbing it.
        let mut pidfile = fs::OpenOptions::new()
            .truncate(false)
            .create(true)
            .read(true)
            .write(true)
            .open(path.as_ref())
            .map_err(PidfileError::CouldNotOpen)?;

        // Now try to acquire an exclusive lock. This will fail if another process or another
        // instance of `Pidfile` is holding a lock onto the same pidfile.
        pidfile.lock_exclusive().map_err(PidfileError::LockFailed);

        // At this point, we're the exclusive users of the file and can read its contents.
        let mut raw_contents = String::new();
        pidfile
            .read_to_string(&mut raw_contents)
            .map_err(PidfileError::ReadFailed)?;

        // Note: We cannot distinguish an empty file from a non-existing file, unfortunately.
        let previous = if raw_contents == "" {
            None
        } else {
            Some(raw_contents.parse().map_err(PidfileError::Corrupted)?)
        };

        let pid = process::id();

        // If we encounter our own PID, we got extremely unlucky, or something went really wrong.
        if previous == Some(pid) {
            return Err(PidfileError::DuplicatedPid);
        }

        // Do our best to ensure that we always have some contents in the file immediately.
        pidfile
            .write_all(pid.to_string().as_bytes())
            .map_err(PidfileError::WriteFailed)?;

        Ok(Pidfile {
            pidfile,
            path: path.as_ref().to_owned(),
            previous,
        })
    }

    /// Whether or not the Pidfile indicated a previously unclean shutdown.
    fn unclean_shutdown(&self) -> bool {
        // If there are any previous contents, we crashed. We check for our own PID already before.
        self.previous.is_some()
    }
}

impl Drop for Pidfile {
    fn drop(&mut self) {
        // When dropping the pidfile, we delete its file. We are still keeping the logs and the
        // opened file handle, which will get cleaned up naturally.
        if let Err(err) = fs::remove_file(&self.path) {
            warn!(path=%self.path.display(), %err, "could not delete pidfile");
        }
    }
}
