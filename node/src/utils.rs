//! Various functions that are not limited to a particular module, but are too small to warrant
//! being factored out into standalone crates.

mod counting_channel;
pub mod ds;
mod external;
pub mod milliseconds;
pub mod pid_file;
#[cfg(target_os = "linux")]
pub(crate) mod rlimit;
mod round_robin;

use std::{
    any,
    cell::RefCell,
    fmt::{self, Debug, Display, Formatter},
    fs,
    io::{self, Write},
    net::{SocketAddr, ToSocketAddrs},
    ops::{Add, BitXorAssign, Div},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
#[cfg(test)]
use std::{env, str::FromStr};

use datasize::DataSize;
use hyper::server::{conn::AddrIncoming, Builder, Server};
use libc::{c_long, sysconf, _SC_PAGESIZE};
use once_cell::sync::Lazy;
use serde::Serialize;
use thiserror::Error;
use tracing::{error, warn};

pub(crate) use counting_channel::{counting_unbounded_channel, CountingReceiver, CountingSender};
#[cfg(test)]
pub use external::RESOURCES_PATH;
pub use external::{External, LoadError, Loadable};
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
        error: Box<dyn std::error::Error + Send + Sync>,
    },
}

pub(crate) fn start_listening(address: &str) -> Result<Builder<AddrIncoming>, ListeningError> {
    let address = resolve_address(address).map_err(|error| {
        warn!(%error, %address, "failed to start HTTP server, cannot parse address");
        ListeningError::ResolveAddress(error)
    })?;

    Server::try_bind(&address).map_err(|error| {
        warn!(%error, %address, "failed to start HTTP server");
        ListeningError::Listen {
            address,
            error: Box::new(error),
        }
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

/// Writes data to `path`, ensuring only the owner can read or write it.
///
/// Otherwise functions like [`write_file`].
pub(crate) fn write_private_file<P: AsRef<Path>, B: AsRef<[u8]>>(
    filename: P,
    data: B,
) -> Result<(), WriteFileError> {
    let path = filename.as_ref();
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(data.as_ref()))
        .map_err(|error| WriteFileError {
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
    pub fn dir(&self) -> &Path {
        self.dir.as_ref()
    }

    /// Deconstructs a with-directory context.
    pub(crate) fn into_parts(self) -> (PathBuf, T) {
        (self.dir, self.value)
    }

    /// Maps an internal value onto a reference.
    pub fn map_ref<U, F: FnOnce(&T) -> U>(&self, f: F) -> WithDir<U> {
        WithDir {
            dir: self.dir.clone(),
            value: f(&self.value),
        }
    }

    /// Get a reference to the inner value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Adds `self.dir` as a parent if `path` is relative, otherwise returns `path` unchanged.
    pub fn with_dir(&self, path: PathBuf) -> PathBuf {
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
    /// This node.
    Ourself,
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
            Source::Client | Source::Ourself => None,
        }
    }
}

impl<I: Display> Display for Source<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Source::Peer(node_id) => Display::fmt(node_id, formatter),
            Source::Client => write!(formatter, "client"),
            Source::Ourself => write!(formatter, "ourself"),
        }
    }
}

/// Divides `numerator` by `denominator` and rounds to the closest integer (round half down).
///
/// `numerator + denominator / 2` must not overflow, and `denominator` must not be zero.
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
}

/// Reads an envvar from the environment and, if present, parses it.
///
/// Only absent envvars are returned as `None`.
///
/// # Panics
///
/// Panics on any parse error.
#[cfg(test)]
pub fn read_env<T: FromStr>(name: &str) -> Option<T>
where
    <T as FromStr>::Err: Debug,
{
    match env::var(name) {
        Ok(raw) => Some(
            raw.parse()
                .unwrap_or_else(|_| panic!("cannot parse envvar `{}`", name)),
        ),
        Err(env::VarError::NotPresent) => None,
        Err(err) => {
            panic!("{:?}", err)
        }
    }
}

/// XORs two byte sequences.
///
/// # Panics
///
/// Panics if `lhs` and `rhs` are not of equal length.
#[inline]
pub fn xor(lhs: &mut [u8], rhs: &[u8]) {
    // Implementing SIMD support is left as an exercise for the reader.
    assert_eq!(lhs.len(), rhs.len(), "xor inputs should have equal length");
    lhs.iter_mut()
        .zip(rhs.iter())
        .for_each(|(sb, &cb)| sb.bitxor_assign(cb));
}

/// Wait until all strong references for a particular arc have been dropped.
///
/// Downgrades and immediately drops the `Arc`, keeping only a weak reference. The reference will
/// then be polled `attempts` times, unless it has a strong reference count of 0.
///
/// Returns whether or not `arc` has zero strong references left.
///
/// # Note
///
/// Using this function is usually a potential architectural issue and it should be used very
/// sparingly. Consider introducing a different access pattern for the value under `Arc`.
pub async fn wait_for_arc_drop<T>(arc: Arc<T>, attempts: usize, retry_delay: Duration) -> bool {
    // Ensure that if we do hold the last reference, we are now going to 0.
    let weak = Arc::downgrade(&arc);
    drop(arc);

    for _ in 0..attempts {
        let strong_count = weak.strong_count();

        if strong_count == 0 {
            // Everything has been dropped, we are done.
            return true;
        }

        tokio::time::sleep(retry_delay).await;
    }

    error!(
        attempts, ?retry_delay, ty=%any::type_name::<T>(),
        "failed to clean up shared reference"
    );

    false
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::{wait_for_arc_drop, xor};

    #[test]
    fn xor_works() {
        let mut lhs = [0x43, 0x53, 0xf2, 0x2f, 0xa9, 0x70, 0xfb, 0xf4];
        let rhs = [0x04, 0x0b, 0x5c, 0xa1, 0xef, 0x11, 0x12, 0x23];
        let xor_result = [0x47, 0x58, 0xae, 0x8e, 0x46, 0x61, 0xe9, 0xd7];

        xor(&mut lhs, &rhs);

        assert_eq!(lhs, xor_result);
    }

    #[test]
    #[should_panic(expected = "equal length")]
    fn xor_panics_on_uneven_inputs() {
        let mut lhs = [0x43, 0x53, 0xf2, 0x2f, 0xa9, 0x70, 0xfb, 0xf4];
        let rhs = [0x04, 0x0b, 0x5c, 0xa1, 0xef, 0x11];

        xor(&mut lhs, &rhs);
    }

    #[tokio::test]
    async fn arc_drop_waits_for_drop() {
        let retry_delay = Duration::from_millis(25);
        let attempts = 15;

        let arc = Arc::new(());

        let arc_in_background = arc.clone();
        let _weak_in_background = Arc::downgrade(&arc);

        // At this point, the Arc has the following refernces:
        //
        // * main test task (`arc`, strong)
        // * background strong reference (`arc_in_background`)
        // * background weak reference (`weak_in_background`)

        // Phase 1: waiting for the arc should fail, because there still is the background
        // reference.
        assert!(!wait_for_arc_drop(arc, attempts, retry_delay).await);

        // We "restore" the arc from the background arc.
        let arc = arc_in_background.clone();

        // Add another "foreground" weak reference.
        let weak = Arc::downgrade(&arc);

        // Phase 2: Our background tasks drops its reference, now we should succeed.
        drop(arc_in_background);
        assert!(wait_for_arc_drop(arc, attempts, retry_delay).await);

        // Immedetialy after, we should not be able to obtain a strong reference anymore.
        // This test fails only if we have a race condition, so false positive tests are possible.
        assert!(weak.upgrade().is_none());
    }
}
